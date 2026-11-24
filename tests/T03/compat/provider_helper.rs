// Isolated protocol fixture only. No model, native account, or host gateway is used.
mod private_gateway {
    use std::{io::{Read,Write},net::TcpListener,process::{Command,Stdio},sync::{Arc,atomic::{AtomicBool,Ordering}},thread,time::{Duration,Instant}};
    pub fn parent_runs_child(test_name:&str)->bool {
        if std::env::var("QROC_PRIVATE_FIXTURE_CHILD").as_deref()==Ok(test_name){return false;}
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();
        let address=listener.local_addr().unwrap();listener.set_nonblocking(true).unwrap();
        let stop=Arc::new(AtomicBool::new(false));let worker_stop=stop.clone();
        let server=thread::spawn(move || {
            let deadline=Instant::now()+Duration::from_secs(10);let mut count=0;
            while !worker_stop.load(Ordering::Relaxed)&&Instant::now()<deadline {
                match listener.accept() {
                    Ok((mut stream,_))=>{
                        stream.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
                        stream.set_write_timeout(Some(Duration::from_millis(500))).unwrap();
                        let mut request=Vec::new();let mut buffer=[0u8;8192];
                        let body_start=loop {
                            let n=stream.read(&mut buffer).unwrap();assert!(n>0);request.extend_from_slice(&buffer[..n]);assert!(request.len()<4*1024*1024);
                            if let Some(i)=request.windows(4).position(|w|w==b"\r\n\r\n"){break i+4;}
                        };
                        let header=std::str::from_utf8(&request[..body_start]).unwrap();
                        assert!(header.starts_with("POST /v1/chat/completions HTTP/1.1"));
                        let length=header.lines().find_map(|line|line.strip_prefix("Content-Length: ")).unwrap().trim().parse::<usize>().unwrap();assert!(length<4*1024*1024);
                        while request.len()-body_start<length {let n=stream.read(&mut buffer).unwrap();assert!(n>0);request.extend_from_slice(&buffer[..n]);}
                        let input:serde_json::Value=serde_json::from_slice(&request[body_start..]).unwrap();assert_eq!(input["model"],"t03-private-protocol-fixture");assert_eq!(input["max_tokens"],1024);
                        let body=serde_json::to_vec(&serde_json::json!({"choices":[{"message":{"content":"Private protocol fixture synthesis draft; no actual inference."},"finish_reason":"stop"}]})).unwrap();
                        write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).unwrap();stream.write_all(&body).unwrap();count+=1;
                    },
                    Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>thread::sleep(Duration::from_millis(5)),
                    Err(e)=>panic!("private fixture accept: {e}")
                }
            }
            count
        });
        let mut child=Command::new(std::env::current_exe().unwrap()).args(["--exact",test_name,"--nocapture","--test-threads=1"])
            .env("QROC_PRIVATE_FIXTURE_CHILD",test_name).env("QROC_GATEWAY",address.to_string()).env("QROC_MODEL","t03-private-protocol-fixture").env("QROC_TIMEOUT_SECONDS","15").env("QROC_MAX_TOKENS","1024")
            .env_remove("SOLITON_QROC_GATEWAY").env_remove("SOLITON_QROC_TIMEOUT_SECONDS").stdin(Stdio::null()).spawn().unwrap();
        let deadline=Instant::now()+Duration::from_secs(10);
        let status=loop {if let Some(status)=child.try_wait().unwrap(){break Some(status)}if Instant::now()>deadline {child.kill().unwrap();child.wait().unwrap();break None}thread::sleep(Duration::from_millis(5));};
        stop.store(true,Ordering::Relaxed);let requests=server.join().unwrap();
        assert!(status.is_some_and(|s|s.success()),"isolated child failed/timed out");assert_eq!(requests,1,"fixture must serve exactly one explicit request");true
    }
    pub fn completed(store:&mut qroc_standalone::qroc::QrocStore,job:&serde_json::Value)->serde_json::Value {
        let deadline=Instant::now()+Duration::from_secs(3);
        loop {let status=store.command("qroc-job-status",&serde_json::json!({"id":job["id"]})).unwrap();if status["state"]=="succeeded"{return status;}assert!(status["state"]=="running", "unexpected fixture state: {status}");assert!(Instant::now()<deadline,"private fixture completion timed out");thread::sleep(Duration::from_millis(5));}
    }
}
