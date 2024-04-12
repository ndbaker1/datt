//! The Sflix website uses a scheme to stream video to the browser by sending segments of the MP4
//! stream data over various looping filetypes which are then loaded into the buffer of the active
//! HTML <video> element.
//!
//! The hope here is that we can parallelize this to be able to download all episodes from a season
//! for any show on sflix.to

use std::{io::Write, ops::Range, path::Path};

use futures::{stream::FuturesOrdered, StreamExt};

use crate::downloader::fetch;

/// Looping ring of extension types that parts of the video buffer are downloaded as.
const EXTENSION_LOOP: &[&'static str] = &["html", "js", "css", "txt", "png", "webp", "ico", "jpg"];

pub async fn run(
    url: String,
    output_file: String,
    batch_size: usize,
    window_count: usize,
) -> crate::Res<()> {
    // create a temporary work directory
    let work_dir = std::path::Path::new(".temp");
    // tracking variables
    let mut index = 1usize;
    let mut ext_index = 0usize;
    // create and ordered parallel set of futures to run fetching tasks
    // which can be refreshed once a single job errors
    let mut tasks = FuturesOrdered::new();
    // kick off the first set of worker tasks
    for i in 0..=window_count {
        tasks.push_back(batch(
            index + i * batch_size..index + (i + 1) * batch_size,
            ext_index,
            &url,
            work_dir,
        ));
    }
    // track consecutive errors to tell when we have reached the end of the video segments
    let mut consecutive_errors = 0;
    // continue reading results containing the markers for the next segment
    // until we get a positive signal
    while let Some(Ok((stuck, index_, ext_index_))) = tasks.next().await {
        // update the index pointers to the most recent success
        index = index_;
        ext_index = ext_index_;
        // test whether the task got stuck on a bad index pair
        if !stuck {
            consecutive_errors += 1;
            if consecutive_errors > batch_size {
                break;
            }
            // if there was an error, then we need to relaunch all of the jobs in the windows size
            // the the new correct base index
            for i in 0..=window_count {
                tasks.push_back(batch(
                    index + i * batch_size..index + (i + 1) * batch_size,
                    ext_index,
                    &url,
                    work_dir,
                ));
            }
        } else {
            consecutive_errors = 0;
            // add another worker to the pool at the next index group
            tasks.push_back(batch(
                index + (window_count) * batch_size..index + (window_count + 1) * batch_size,
                ext_index,
                &url,
                work_dir,
            ));
        }

        println!("task count: {}", tasks.len());
    }

    // concatenate all of the files into one final archive
    let mut output = std::fs::File::create(output_file)?;
    let mut directory_list = std::fs::read_dir(work_dir)?
        .map(|s| s.unwrap())
        .collect::<Vec<_>>();
    directory_list.sort_by(|a, b| {
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

async fn batch<P>(
    range: Range<usize>,
    ext_offset: usize,
    base_url: &str,
    work_dir: P,
) -> crate::Res<(bool, usize, usize)>
where
    P: AsRef<Path>,
{
    let end = range.end;
    let mut ext_index = ext_offset;
    for index in range {
        let segment = segment_name(index, ext_index);
        let segment_path = work_dir.as_ref().join(format!("{:08}", index));
        if segment_path.exists() {
            println!("segment {segment} already exists");
            ext_index = (ext_index + 1).rem_euclid(EXTENSION_LOOP.len());
            continue;
        }
        println!("processing {segment}");
        if let Ok(buf) = fetch(&format!("{}/{}", base_url, segment)).await {
            std::fs::create_dir_all(work_dir.as_ref())?;
            std::fs::File::create(segment_path)?.write_all(&buf)?;
            ext_index = (ext_index + 1).rem_euclid(EXTENSION_LOOP.len());
        } else {
            println!("no value for {segment}");
            return Ok((false, index, ext_index));
        }
    }
    Ok((true, end, ext_index))
}

fn segment_name(index: usize, ext_index: usize) -> String {
    format!("seg-{}-v1-a1.{}", index, EXTENSION_LOOP[ext_index])
}
