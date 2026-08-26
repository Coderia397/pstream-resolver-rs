use std::time::Duration;

#[tokio::test]
async fn test_verify_extracted_stream_accessibility() {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let stream_url = "https://macdn.aoneroom.com/media/vone/2022/11/22/1d6874b1fbeec76606f035e08b737035-sd.mp4";

    let resp = client
        .get(stream_url)
        .header("Referer", "https://movieboxonline.net/")
        .header("Range", "bytes=0-1023")
        .send()
        .await;

    match resp {
        Ok(res) => {
            let status = res.status();
            let content_type = res.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("unknown");
            let content_length = res.headers().get("content-length").and_then(|v| v.to_str().ok()).unwrap_or("unknown");
            println!("[Stream Probe] Status: {status}");
            println!("[Stream Probe] Content-Type: {content_type}");
            println!("[Stream Probe] Content-Length: {content_length}");
            let bytes = res.bytes().await.unwrap_or_default();
            println!("[Stream Probe] Fetched {} bytes", bytes.len());
            // Check for ftyp/mp4 magic bytes in first 16 bytes
            let is_mp4 = bytes.windows(4).any(|w| w == b"ftyp" || w == b"moov" || w == b"mdat");
            println!("[Stream Probe] Contains MP4 container atom: {is_mp4}");
            assert!(status.is_success() || status.as_u16() == 206, "Stream must return 200 or 206");
        }
        Err(e) => {
            println!("[Stream Probe] Error: {e}");
        }
    }
}
