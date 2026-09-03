use crate::models::Source;
use reqwest::Client;
use std::time::{SystemTime, UNIX_EPOCH};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use wasmi::{Engine, Linker, Module, Store};
use regex::Regex;
use serde_json::Value;
use std::error::Error;

pub async fn extract_m3u8(client: &Client, src_url: &str) -> Result<Vec<Source>, Box<dyn Error>> {
    // 1. Get outer embed JSON
    let src_resp = client.get(src_url)
        .header("Referer", "https://1shows.org/")
        .send().await?
        .json::<Value>().await?;
        
    let embed_url = src_resp["src"].as_str()
        .ok_or("Missing src in vsembed response")?;

    // 2. Get inner player URL and metaApi with VS token
    let embed_html = client.get(embed_url)
        .header("Referer", "https://vsembed.ru/")
        .send().await?
        .text().await?;
        
    let re_player = Regex::new(r#""playerUrl":"([^"]+)""#).unwrap();
    let player_path_raw = re_player.captures(&embed_html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .ok_or("Could not find playerUrl in CFG")?;
    let player_path = player_path_raw.replace("\\u0026", "&");
        
    let re_vs = Regex::new(r"vs=([A-Za-z0-9_\-]+)").unwrap();
    let vs_token = re_vs.captures(&player_path)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .ok_or("Could not extract VS token from playerUrl")?;

    let re_meta = Regex::new(r#""metaApi":"([^"]+)""#).unwrap();
    let meta_api_raw = re_meta.captures(&embed_html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .ok_or("Could not find metaApi in CFG")?;
    let meta_api = meta_api_raw.replace("\\u0026", "&");

    // 3. Hit the stream API by appending &stream_urls and vs=
    let api_url = if meta_api.contains('?') {
        format!("{}&stream_urls&vs={}", meta_api, vs_token)
    } else {
        format!("{}?stream_urls&vs={}", meta_api, vs_token)
    };

    
    
    let api_resp = client.get(&api_url)
        .header("Referer", "https://cloudorchestranova.com/")
        .send().await?
        .json::<Value>().await?;
        
    let data = &api_resp["data"];
    
    // Vidsrc returns fake trailers renamed as "SADiESiNK" cam-rips for unreleased
    // movies or titles they don't have. We must filter these out so they don't
    // override valid sources from other providers.
    if let Some(file_name) = data["file_name"].as_str() {
        if file_name.contains("SADiESiNK") {
            return Err("fake trailer detected (SADiESiNK)".into());
        }
    }
    
    let stream_urls_val = &data["stream_urls"];
    
    if let Some(arr) = stream_urls_val.as_array() {
        let mut sources = Vec::new();
        for url_val in arr {
            if let Some(url) = url_val.as_str() {
                if !url.is_empty() {
                    sources.push(Source::direct_m3u8(url.to_string(), "auto".to_string()));
                }
            }
        }
        if !sources.is_empty() {
            return Ok(sources);
        }
    }
    
    let enc_str = stream_urls_val.as_str()
        .ok_or("stream_urls is not a string or array")?;
        
    let w_str = api_resp["vs"]["w"].as_i64().map(|v| v.to_string());
    let w = api_resp["vs"]["w"].as_str().or(w_str.as_deref()).unwrap_or("0");
        
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    
    // 4. Get WASM
    let wasm_url = format!("https://data.vidsrcme.ru/wasm.php?w={}&_={}", w, ts);
    let wasm_bytes = client.get(&wasm_url)
        .header("Referer", "https://cloudorchestranova.com/")
        .send().await?
        .bytes().await?;
        
    // 5. Decrypt using wasmi
    let enc_bytes = STANDARD.decode(enc_str)?;
    
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm_bytes)
        .map_err(|e| format!("WASM compile error: {}", e))?;
        
    type HostState = ();
    let mut store = Store::new(&engine, ());
    let linker = <Linker<HostState>>::new(&engine);
    
    let instance = linker.instantiate(&mut store, &module)
        .map_err(|e| format!("WASM instantiate error: {}", e))?
        .start(&mut store)
        .map_err(|e| format!("WASM start error: {}", e))?;
        
    let alloc = instance.get_typed_func::<i32, i32>(&store, "alloc")
        .map_err(|e| format!("No alloc func: {}", e))?;
    let decrypt = instance.get_typed_func::<(i32, i32), i32>(&store, "decrypt")
        .map_err(|e| format!("No decrypt func: {}", e))?;
    let memory = instance.get_memory(&store, "memory")
        .ok_or("No memory export")?;
        
    let ptr = alloc.call(&mut store, enc_bytes.len() as i32)
        .map_err(|e| format!("alloc call failed: {}", e))?;
        
    memory.write(&mut store, ptr as usize, &enc_bytes)
        .map_err(|e| format!("memory write failed: {}", e))?;
        
    let out_len = decrypt.call(&mut store, (ptr, enc_bytes.len() as i32))
        .map_err(|e| format!("decrypt call failed: {}", e))?;
        
    let mut result_bytes = vec![0; out_len as usize];
    memory.read(&mut store, (ptr + 12) as usize, &mut result_bytes)
        .map_err(|e| format!("memory read failed: {}", e))?;
        
    let result = String::from_utf8_lossy(&result_bytes);
    
    let mut sources = Vec::new();
    for url in result.split('\n') {
        let url = url.trim();
        if !url.is_empty() {
            sources.push(Source::direct_m3u8(url.to_string(), "auto".to_string()));
        }
    }
    
    Ok(sources)
}
