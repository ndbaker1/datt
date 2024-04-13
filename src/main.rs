use clap::{Parser, Subcommand};

mod downloader;

#[derive(Parser)]
#[clap(about = r#"
 _____     ______    ______   ______  
/\  __ \  /\  __ \  /\__  _\ /\__  _\ 
\ \ \/\ \ \ \  __ \ \/_/\ \/ \/_/\ \/ 
 \ \____-  \ \_\ \_\   \ \_\    \ \_\ 
  \/____/   \/_/\/_/    \/_/     \/_/ 

  Download All The Things.
"#)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    Gogoanime {
        /// Url of the anime
        #[arg(short, long)]
        url: String,
        /// Captcha for the download website
        #[arg(short, long)]
        captcha: String,
        /// output directory
        #[arg(short, long, default_value_t = String::from("gogoanime-parts"))]
        output_dir: String,
    },
    Sflix {
        /// Url of the segment path
        #[arg(short, long)]
        url: String,
        /// Output file name (should be mp4)
        #[arg(short, long)]
        output_file: String,
        /// Number of segments to try downloading in one batch
        #[arg(short, long, default_value_t = 10)]
        batch_size: usize,
        /// Number of batches to run in parallel
        #[arg(short, long, default_value_t = 30)]
        parallel: usize,
    },
}

#[tokio::main]
async fn main() -> Res<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Sflix {
            url,
            output_file,
            batch_size,
            parallel,
        } => downloader::sflix::run(&url, &output_file, batch_size, parallel).await?,
        Commands::Gogoanime {
            url,
            captcha,
            output_dir,
        } => downloader::gogoanime::run(&url, &captcha, &output_dir, 5).await?,
    }

    Ok(())
}

pub(crate) type Res<T> = Result<T, Box<dyn std::error::Error>>;
