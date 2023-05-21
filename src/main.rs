// #![allow(dead_code, unused_variables, unused_imports)]
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use env_logger::Env;
use log::debug;

use crate::obs::ClientOptions;

mod obs;


#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value_os_t = PathBuf::from("config.toml"))]
    config: PathBuf,
}


fn main() -> Result<()> {
    env_logger::init_from_env(Env::default().default_filter_or("info"));

    let args = Args::parse();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let client = rt.block_on(async {
        let config = tokio::fs::read_to_string(args.config)
            .await
            .expect("Read config file failed");

        let config: ClientOptions = toml::from_str(&config).expect("Parse config file failed");

        debug!("options: {:?}", config);

        let client = obs::Client::new(config).await;

        client
    })?;

    client.run()?;

    Ok(())
}
