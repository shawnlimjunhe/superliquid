use serde::de::DeserializeOwned;
use tokio::{ io::{ AsyncReadExt, AsyncWriteExt }, net::TcpStream };

use serde_json;

const LEN_BUF_LEN: usize = 4;

pub async fn send_data(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    let data_len = (data.len() as u32).to_be_bytes();
    let _ = stream.write_all(&data_len).await;
    let _ = stream.write_all(data).await;
    Ok(())
}

pub async fn receive_data(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; LEN_BUF_LEN];

    stream.read_exact(&mut len_buf).await?;

    let msg_len = u32::from_be_bytes(len_buf) as usize;

    let mut resp_buf = vec![0u8; msg_len];

    stream.read_exact(&mut resp_buf).await?;

    Ok(resp_buf)
}

pub async fn receive_json<T: DeserializeOwned>(stream: &mut TcpStream) -> std::io::Result<T> {
    let raw_bytes = receive_data(stream).await?;

    let parsed = serde_json
        ::from_slice::<T>(&raw_bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    Ok(parsed)
}
