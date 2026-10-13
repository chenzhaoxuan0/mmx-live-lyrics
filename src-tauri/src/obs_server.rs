//! OBS 浏览器源服务：单端口同时提供
//!   GET /            → 与桌面窗口相同的歌词页面（含状态与外观）
//!   Upgrade: websocket → 推送 text/status/settings 消息
//!
//! 只绑 127.0.0.1，不出网。HTTP 响应手工写头（无第三方 HTTP 依赖）。
use crate::bus::Bus;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;
use tungstenite::{accept, Message};

pub fn serve(port: u16, bus: Arc<Bus>) {
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => l,
            Err(e) => {
                bus.status("error", &format!("OBS 端口 {port} 绑定失败：{e}"));
                return;
            }
        };
        println!("OBS browser source: http://127.0.0.1:{port}/");
        for conn in listener.incoming() {
            let Ok(stream) = conn else { continue };
            let bus = bus.clone();
            std::thread::spawn(move || {
                let _ = handle(stream, bus);
            });
        }
    });
}

fn handle(mut stream: TcpStream, bus: Arc<Bus>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = Vec::with_capacity(2048);
    let mut tmp = [0u8; 2048];
    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 64 * 1024 {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf).to_lowercase();
    if head.contains("upgrade: websocket") {
        // WebSocket：把已读的请求头回放给 tungstenite 做握手
        let _ = stream.set_read_timeout(Some(Duration::from_millis(40)));
        let mut ws = match accept(PrefixedStream::new(stream, buf)) {
            Ok(ws) => ws,
            Err(_) => return Ok(()),
        };
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        bus.add_client(tx);
        loop {
            match ws.read() {
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(e) => match e {
                    tungstenite::Error::Io(ref io)
                        if io.kind() == std::io::ErrorKind::WouldBlock
                            || io.kind() == std::io::ErrorKind::TimedOut => {}
                    _ => break,
                },
            }
            let mut failed = false;
            while let Ok(json) = rx.try_recv() {
                if ws.send(Message::Text(json.into())).is_err() {
                    failed = true;
                    break;
                }
            }
            if failed {
                break;
            }
        }
        Ok(())
    } else {
        // 普通 HTTP：返回与桌面窗口相同的页面
        let body = include_str!("../../ui/index.html");
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(resp.as_bytes())?;
        stream.write_all(body.as_bytes())?;
        Ok(())
    }
}

/// 先回放已读字节（HTTP 请求头），读尽后透传底层流。
/// tungstenite::accept 需要自己从流中读取握手请求，而我们已经把头消费掉了。
struct PrefixedStream {
    prefix: Vec<u8>,
    pos: usize,
    inner: TcpStream,
}

impl PrefixedStream {
    fn new(inner: TcpStream, prefix: Vec<u8>) -> Self {
        Self { prefix, pos: 0, inner }
    }
}

impl Read for PrefixedStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos < self.prefix.len() {
            let n = (self.prefix.len() - self.pos).min(buf.len());
            buf[..n].copy_from_slice(&self.prefix[self.pos..self.pos + n]);
            self.pos += n;
            return Ok(n);
        }
        self.inner.read(buf)
    }
}

impl Write for PrefixedStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
