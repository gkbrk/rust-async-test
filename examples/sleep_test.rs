use leo_async;

async fn short() {
    leo_async::sleep_ms(250).await;
    println!("Short!");
}

async fn long() {
    leo_async::sleep_ms(700).await;
    println!("Long!");
}

async fn short_long() {
    let start = std::time::Instant::now();
    loop {
        short().await;
        short().await;
        short().await;
        long().await;

        if start.elapsed().as_secs() > 3 {
            break;
        }
    }
}

async fn yor_mama() {
    let start = std::time::Instant::now();

    loop {
        println!("Your mama is so fat, when she sat on an iPod, she made the iPad!");
        leo_async::sleep_ms(300).await;

        if start.elapsed().as_secs() > 3 {
            break;
        }
    }

    leo_async::spawn(async {
        leo_async::sleep_ms(500).await;
        println!("We're done here!");
    });
}

async fn async_main() {
    println!("Hello, world!");

    let short_long = short_long();
    let yor_mama = yor_mama();

    leo_async::select_two(short_long, yor_mama).await;
}

fn main() {
    let res = leo_async::run_main(async_main());
    println!("Result: {:?}", res);
}
