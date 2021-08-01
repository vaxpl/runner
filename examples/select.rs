use async_std::prelude::*;
use async_std::process::{Command, Stdio};
use async_std::task::{self, JoinHandle};
use futures::future::FutureExt;
use futures::pin_mut;
use futures::select;
use std::time::Duration;
use encoding_rs::{Encoding, Encoder};

async fn task1() -> usize {
    let _ = task::sleep(Duration::from_millis(20000)).await;
    1000
}

async fn task2() -> String {
    let _ = task::sleep(Duration::from_millis(10000)).await;
    "Hello".to_owned()
}

fn main() {
    task::block_on(async {
        // let mut t1 = Box::pin(task::spawn(task1()).fuse());
        // let mut t2 = Box::pin(task::spawn(task2()).fuse());
        // let t2 = task2().fuse();
        // pin_mut!(t1, t2);
        let cmd = Command::new("cmd")
            .args(&["/c", "dir", "/s", "*.dll", "c:"])
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stderr = cmd.stderr.unwrap();
        let mut stdout = cmd.stdout.unwrap();
        let t1 = task::spawn(async move {
            loop {
                let mut buffer: [u8; 1024] = [0; 1024];
                match stderr.read(&mut buffer).await {
                    Ok(len) if len > 0 => {
                        if let Some()
                        println!("Err={}", &buffer[..len].len());
                    }
                    _ => { break; }
                }
            }
        }).fuse();
        let t2 = task::spawn(async move {
            loop {
                let mut buffer: [u8; 1024] = [0; 1024];
                match stdout.read(&mut buffer).await {
                    Ok(len) if len > 0 => {
                        println!("Out={}", &buffer[..len].len());
                    }
                    _ => { break; }
                }
            }
        }).fuse();
        pin_mut!(t1, t2);

        // 'outer: loop {
            // let mut s1 = Vec::new();
            // let mut s2 = Vec::new();
        
            // let f1 = stderr.read_to_end(&mut s1).fuse();
            // let f2 = stdout.read_to_end(&mut s2).fuse();
            // pin_mut!(f1, f2);
    
            'inner: loop {
                select! {
                    () = t1 => {
                        println!("Err={:?}", "a");
                        // s1.clear();
                    },
                    () = t2 => {
                        println!("Out={:?}", "b");
                        // s2.clear();
                    },
                    complete => {
                        println!("Complete");
                        break 'inner;
                    }
                    // default => {
                        // task::sleep(Duration::from_millis(10000));
                    // }
                }
            }
        // }
    });
}
