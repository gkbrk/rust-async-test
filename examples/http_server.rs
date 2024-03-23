use std::collections::VecDeque;
use std::io::Write;

struct AsyncBufReader {
    stream: leo_async::tcp::TcpStream,
    buf: VecDeque<u8>,
}

impl AsyncBufReader {
    pub fn new(stream: &mut leo_async::tcp::TcpStream) -> Self {
        Self {
            stream: stream.clone(),
            buf: VecDeque::new(),
        }
    }

    async fn read_underlying(&mut self) -> leo_async::DSSResult<()> {
        let mut rd = [0u8; 4096];
        let n = self.stream.read(&mut rd).await?;
        if n == 0 {
            return Err("EOF".into());
        }

        self.buf.extend(rd.iter().take(n));
        Ok(())
    }

    pub async fn read1(&mut self) -> leo_async::DSSResult<u8> {
        match self.buf.pop_front() {
            Some(c) => Ok(c),
            None => {
                self.read_underlying().await?;
                match self.buf.pop_front() {
                    Some(c) => Ok(c),
                    None => panic!("read_underlying returned 0 bytes but no Error"),
                }
            }
        }
    }
}

async fn read_until_crlf(
    reader: &mut AsyncBufReader,
    target: &mut String,
) -> leo_async::DSSResult<()> {
    target.clear();

    let mut prev = '\0';

    loop {
        let c = reader.read1().await?;

        if prev == '\r' && c as char == '\n' {
            target.pop();
            return Ok(());
        }

        target.push(c as char);
        prev = c as char;
    }
}

async fn write_response(
    stream: &mut leo_async::tcp::TcpStream,
    resp: &[u8],
    vec_buf: &mut Vec<u8>,
) -> leo_async::DSSResult<()> {
    vec_buf.clear();
    write!(vec_buf, "HTTP/1.1 200 OK\r\n")?;
    write!(vec_buf, "Content-Length: {}\r\n", resp.len())?;
    write!(vec_buf, "\r\n")?;

    vec_buf.extend_from_slice(resp);

    stream.write(vec_buf).await?;

    Ok(())
}

async fn handle_connection(stream: std::net::TcpStream) -> leo_async::DSSResult<()> {
    let mut leo_stream = leo_async::tcp::TcpStream::new_with_global_executor(stream);
    let mut buf_reader = AsyncBufReader::new(&mut leo_stream);

    let mut linebuf = String::with_capacity(512);
    let mut vec_buf = Vec::with_capacity(4096);

    loop {
        read_until_crlf(&mut buf_reader, &mut linebuf).await?;

        loop {
            read_until_crlf(&mut buf_reader, &mut linebuf).await?;
            if linebuf.is_empty() {
                break;
            }
        }

        let response = b"Hello, World!\n";
        write_response(&mut leo_stream, response, &mut vec_buf).await?;
    }
}

async fn async_main() -> leo_async::DSSResult<()> {
    let addr = "127.0.0.1:1212";
    let listener = std::net::TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;

    println!("Listening on {}", addr);

    loop {
        let stream = leo_async::tcp::tcpaccept(&listener).await?;
        leo_async::spawn(async move {
            println!("Got a connection!");
            let res = handle_connection(stream).await;
            println!("Connection closed: {:?}", res);
        });
    }
}

fn main() {
    let res = leo_async::run_main(async_main());
    println!("Result: {:?}", res);
}
