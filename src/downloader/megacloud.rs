//! The MegaCloud stream source (for example on the sflix website) uses a scheme to stream video to
//! the browser by sending segments of the MP4 stream data over various looping filetypes which are
//! then loaded into the buffer of the active HTML <video> element.
//!
//! The steps involved are as follows:
//! 1. determine a chunk lenth and start X number of chunk processing tasks spaced apart by the
//!    chunk length.
//! 2. once a chunk processor finishes running, then a new one will be spawned with the next
//!    highest starting chunk index.
//! 3. repeat until the chunks making up the entire movie are downloaded
//!
//! The hope here is that we can parallelize this to be able to download all episodes from a season
//! for any show on sflix.to

use std::{io::Write, path::Path};

use futures::{stream::FuturesUnordered, StreamExt};

use crate::downloader::fetch;

/// Looping ring of extension types that parts of the video buffer are downloaded as.
const EXTENSION_LOOP: &[&'static str] = &["ts"];
// seems like this changed over time.
// const EXTENSION_LOOP: &[&'static str] = &["html", "js", "css", "txt", "png", "webp", "ico", "jpg"];

// Example URL:
// https://wwwe.sesnopser.net/_v11/3e0d3367f3445c73b8373eaa213a6743355688f919acc88b1593a92fe83f772755cca290f4c0a3e2744530f93087cce2d00924e853c9e40615d95a0d2ae45d2dc3addb2045e324e4ba16a586a87146acc97697046e5211870ee2ae60f118d088f8edc2c4f067082713be91326171f50d29a8d6720a3cc1035766141151bfe522/720/
pub async fn run(
    url: &str,
    output_file: &str,
    chunk_size: usize,
    chunk_parallel: usize,
) -> crate::Res<()> {
    // create a temporary work directory.
    let work_dir = format!(".temp-{}", output_file);
    let work_dir = std::path::Path::new(&work_dir);

    // create an unordered parallel set of futures to run fetching tasks.
    let mut tasks = FuturesUnordered::new();

    // enqueue the first chunk of segments to start processing.
    for i in 0..=chunk_parallel {
        tasks.push(stream_chunks(
            i,
            chunk_size,
            url.to_string(),
            None,
            work_dir,
        ));
    }

    let mut chunk_index = chunk_parallel;
    // continue reading results containing the markers for the next segment
    while let Some(task) = tasks.next().await {
        if let Ok((last_chunk_index, last_ext_index)) = task {
            // update the index pointers to the most recent success
            chunk_index = chunk_index.max(last_chunk_index) + 1;
            // the the new correct base index
            tasks.push(stream_chunks(
                chunk_index,
                chunk_size,
                url.to_string(),
                Some(last_ext_index),
                work_dir,
            ));

            eprintln!("[INFO] chunk index: {} completed.", last_chunk_index);
            eprintln!("[INFO] task count: {}", tasks.len());
        }
    }

    // concatenate all of the files into one final archive
    let mut output = std::fs::File::create(output_file)?;
    let mut directory_list = std::fs::read_dir(work_dir)?
        .map(|s| s.unwrap())
        .collect::<Vec<_>>();
    directory_list.sort_by(|a, b| {
        // not super happy about this, but it works
        a.file_name()
            .into_string()
            .unwrap()
            .parse::<usize>()
            .unwrap()
            .cmp(
                &b.file_name()
                    .into_string()
                    .unwrap()
                    .parse::<usize>()
                    .unwrap(),
            )
    });
    for entry in directory_list {
        let mut input = std::fs::File::open(entry.path())?;
        std::io::copy(&mut input, &mut output)?;
    }

    // delete the temporary work directory
    std::fs::remove_dir_all(work_dir)?;

    Ok(())
}

async fn stream_chunks<D, S>(
    chunk_index: usize,
    chunk_size: usize,
    url: S,
    ext_hint: Option<usize>,
    work_dir: D,
) -> crate::Res<(usize, usize)>
where
    D: AsRef<Path>,
    S: AsRef<str>,
{
    enum Mode {
        Norm,
        ExtSearch(usize),
    }
    // find the starting index for the chunk.
    // need to iterate over the possible extensions and backtracking is ok up to an upper limit
    let mut idx = chunk_index * chunk_size;
    let mut extension_index = ext_hint.unwrap_or(0);
    let mut mode = Mode::Norm;
    loop {
        if idx >= (chunk_index + 1) * chunk_size {
            break;
        }

        let segment = segment_name(idx, extension_index);
        let segment_path = work_dir.as_ref().join(format!("{:08}", idx));

        println!("processing {segment}");

        if segment_path.exists() {
            println!("segment {segment} already exists, skipping.");
            extension_index = (extension_index + 1).rem_euclid(EXTENSION_LOOP.len());
            idx += 1;
            continue;
        }

        let segment_url = generate_url(url.as_ref(), &segment);
        if let Ok(buf) = fetch(&segment_url).await {
            std::fs::create_dir_all(work_dir.as_ref())?;
            std::fs::File::create(segment_path)?.write_all(&buf)?;
            mode = Mode::Norm;
            extension_index = (extension_index + 1).rem_euclid(EXTENSION_LOOP.len());
            idx += 1;
        } else {
            println!("no value for {segment}");
            if let Mode::ExtSearch(ref mut search_counter) = mode {
                *search_counter += 1;
                if *search_counter >= EXTENSION_LOOP.len() {
                    // we exit this loop because we dont have any new extensions that we can try.
                    return Err("finished searching loop".into());
                }
            } else {
                mode = Mode::ExtSearch(0);
            }
        }
    }

    Ok((chunk_index, extension_index))
}

fn generate_url(url: &str, segment: &str) -> String {
    format!("{}/{}", url, segment)
}

fn segment_name(index: usize, ext_index: usize) -> String {
    format!("seg-{}-v1-a1.{}", index, EXTENSION_LOOP[ext_index])
}
