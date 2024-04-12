//! The gogoanime downloader has to resovle through several phases:
//! 1. find all current releases episodes by iterating through calls to the frontend and testing
//!    for the existence of "not found" messages like "404" or "Page not found"
//! 2. search the HTML for the download link, which is known to be embtaku.pro, and the ID of the
//!    anime that is used to query the download endpoint
//! 3. query the download endpoint using the show ID a provided captcha token, then match on
//!    download URL which corresponds to the video resolution we want to download. (ex. 720p)

use std::{collections::HashMap, io::Write};

use futures::{stream::FuturesOrdered, StreamExt};

use crate::downloader::fetch;

pub async fn run(url: &str, captcha: &str, output_dir: &str) -> crate::Res<()> {
    // create a temporary work directory
    let base = url
        .split("-")
        .collect::<Vec<_>>()
        .split_last()
        .ok_or(format!("couldn't split url '{}'", url))?
        .1
        .join("-");

    let stream_regex = regex::Regex::new(
        r"https://embtaku.pro/download\?id=(.*)\&typesub=Gogoanime-SUB\&title=.*\+Episode\+\d+",
    )?;
    let download_regex = regex::Regex::new(
        r##"(https://gredirect.info/download.php\?url=.*)" download.*Download.*[\W]*720P"##,
    )?;

    // create a folder to hold the saved videos
    let output_dir_path = std::path::Path::new(output_dir);
    std::fs::create_dir_all(output_dir_path)?;

    // start searching at the first episode
    let mut ep = 0usize;
    // create and ordered parallel set of futures to run fetching tasks
    // which can be refreshed once a single job errors
    let mut tasks = FuturesOrdered::new();
    loop {
        // bump the episode counter
        ep += 1;
        // check if the file exists before proceeding
        // this helps caching when rerunning after a failed pull
        let filename = format!("{:02}.mp4", ep.clone());
        let output_file_path = output_dir_path.join(&filename);
        if std::path::Path::new(&output_file_path).exists() {
            println!("skipping episode {ep} since it already exists");
            continue;
        }
        // format the gogoanime url with the episode
        let episode_url = format!("{}-{}", base, ep);
        // The 'Page not found' message indicates if there are any more episodes to download
        let response_text = reqwest::get(episode_url).await?.text().await?;
        if response_text.contains("Page not found") {
            break;
        }
        // read the entire url of the download website and save the ID to use when querying the
        // download endpoints. Also need the captcha but that is handled ahead of time.
        let matches = &stream_regex.captures(&response_text).unwrap();
        let (stream_url, id) = (&matches[0], &matches[1]);
        // create payload for the POST call toi the download endpoint which returns more XML
        // containing links for all the downloads with differing resolutions
        let params = {
            let mut params = HashMap::new();
            params.insert("captcha_v3", captcha.to_string());
            params.insert("id", id.to_string());
            params
        };
        // using a regex to match the resolution we want, get the download URL
        let download_url = download_regex
            .captures(
                &reqwest::Client::new()
                    .post(stream_url)
                    .form(&params)
                    .send()
                    .await?
                    .text()
                    .await?,
            )
            .unwrap()[1]
            .to_owned();

        // add an async task to download in this epsodes's video
        println!("queueing download for episode {} [{}]", ep, download_url);
        tasks.push_back(async move {
            for i in 0..3 {
                if let Ok(buf) = fetch(&download_url).await {
                    std::fs::File::create(&output_file_path)?.write_all(&buf)?;
                    println!("saved episode {ep} to {output_file_path:?}");
                    break;
                }
                eprintln!("failed to download episode {ep}. (attempt {i})");
            }
            crate::Res::<()>::Ok(())
        });
    }
    // run all tasks to completion
    tasks.count().await;
    Ok(())
}
