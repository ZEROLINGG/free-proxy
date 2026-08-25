
mod cs;
mod test;
mod web;

use std::time::Duration;
use anyhow::Result;
use tokio::time::sleep;
use crate::cs::{Client, Server};
use crate::test::base::*;
use crate::test::print_report;
use crate::web::WebServer;

#[tokio::main]
async fn main() -> Result<()> {
    let mut server = Server::new()?;
    server.start().await?;

    let mut client = Client::new(server.key().unwrap())?;
    client.start().await?;

    let web = WebServer::new();
    web.start().await?;


    let _ = dbg!(client.check_availability().await);
    sleep(Duration::from_secs(5)).await;
    let mut results = Vec::new();

    test_fn!(proxy_example_com_http, results);
    test_fn!(proxy_example_com_https, results);
    test_fn!(proxy_localhost_hello, results);




    print_report(results);

    Ok(())
}


