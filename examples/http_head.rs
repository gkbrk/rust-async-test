use leo_async;

async fn head_request(addr: &str, host: &str, path: &str) -> leo_async::DSSResult<String> {
    let conn = leo_async::tcp::tcpdial(addr, 80).await?;

    println!("Connected to {}", addr);

    conn.write(format!("HEAD {} HTTP/1.1\r\n", path).as_bytes())
        .await?;
    conn.write(format!("Host: {}\r\n", host).as_bytes()).await?;
    conn.write(b"Connection: close\r\n").await?;
    conn.write(b"\r\n").await?;

    let mut response = String::new();
    loop {
        let mut buf = vec![0u8; 512];
        let n = conn.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let s = std::str::from_utf8(&buf[..n])?;
        response.push_str(s);

        if response.len() > 4096 {
            break;
        }
    }

    Ok(response)
}

async fn async_main() -> leo_async::DSSResult<()> {
    println!("Going to make some HTTP HEAD requests...");

    let example_com = head_request("example.com", "example.com", "/");
    let gkbrk_com = head_request("gkbrk.com", "gkbrk.com", "/");
    let google_com = head_request("google.com", "google.com", "/");

    let (example_response, gkbrk_response, google_response) =
        leo_async::join3futures(example_com, gkbrk_com, google_com).await;

    println!("Example.com response:\n\n{}\n", example_response?);
    println!("Gkbrk.com response:\n\n{}\n", gkbrk_response?);
    println!("Google.com response:\n\n{}\n", google_response?);

    Ok(())
}

fn main() {
    let res = leo_async::run_main(async_main());
    println!("Result: {:?}", res);
}
