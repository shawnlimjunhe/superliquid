use serde::de::DeserializeOwned;
use tokio::io::{ AsyncReadExt, AsyncWriteExt, AsyncWrite, AsyncRead };

use serde_json;

const LEN_BUF_LEN: usize = 4;

pub async fn send_data<W: AsyncWrite + Unpin>(stream: &mut W, data: &[u8]) -> std::io::Result<()> {
    let data_len = (data.len() as u32).to_be_bytes();
    let _ = stream.write_all(&data_len).await;
    let _ = stream.write_all(data).await;
    Ok(())
}

pub async fn receive_data<W: AsyncRead + Unpin>(stream: &mut W) -> std::io::Result<Vec<u8>> {
    let mut len_buf: [u8; 4] = [0u8; LEN_BUF_LEN];

    stream.read_exact(&mut len_buf).await?;

    let msg_len = u32::from_be_bytes(len_buf) as usize;

    let mut resp_buf = vec![0u8; msg_len];

    stream.read_exact(&mut resp_buf).await?;

    Ok(resp_buf)
}

pub async fn receive_json<T: DeserializeOwned, W: AsyncRead + Unpin>(
    stream: &mut W
) -> std::io::Result<T> {
    let raw_bytes: Vec<u8> = receive_data(stream).await?;

    let parsed = serde_json
        ::from_slice::<T>(&raw_bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{ duplex, AsyncReadExt };
    use std::io::Result;

    async fn test_send_data_helper(payload: &[u8]) -> Result<()> {
        let expected_len = payload.len();

        // Create an in-memory stream pair (like a pipe)
        let (mut client_end, mut server_end) = duplex(1024);

        let expected_payload = payload.to_vec(); // clone to move into task
        let read_task = tokio::spawn(async move {
            let mut len_buf = [0u8; 4];
            server_end.read_exact(&mut len_buf).await.unwrap();
            let len = u32::from_be_bytes(len_buf);
            assert_eq!(len as usize, expected_len);

            let mut data_buf = vec![0u8; len as usize];
            server_end.read_exact(&mut data_buf).await.unwrap();
            assert_eq!(data_buf, expected_payload);
        });

        send_data(&mut client_end, payload).await?;

        // Wait for the read task to finish
        read_task.await.unwrap();
        Ok(())
    }

    async fn test_receive_data_helper(payload: &[u8]) -> Result<()> {
        // Create an in-memory stream pair (like a pipe)
        let (mut client_end, mut server_end) = duplex(1024);

        let payload_to_send = payload.to_vec();

        let expected_len = payload_to_send.len();
        // Spawn a task that writes data to recieve_data
        tokio::spawn(async move {
            let data_len = (payload_to_send.len() as u32).to_be_bytes();
            let _ = client_end.write_all(&data_len).await;
            let _ = client_end.write_all(&payload_to_send).await;
        }).await?;

        let recieved_payload = receive_data(&mut server_end).await?;

        assert_eq!(recieved_payload.len(), expected_len);
        assert_eq!(recieved_payload, payload);

        Ok(())
    }

    #[tokio::test]
    async fn test_send_data() -> Result<()> {
        test_send_data_helper(b"hello world").await
    }

    #[tokio::test]
    async fn test_send_empty_data() -> Result<()> {
        test_send_data_helper(b"").await
    }

    #[tokio::test]
    async fn test_receive_data() -> Result<()> {
        test_receive_data_helper(b"hello world").await
    }

    #[tokio::test]
    async fn test_receive_empty_data() -> Result<()> {
        test_receive_data_helper(b"").await
    }

    #[tokio::test]
    async fn test_receive_data_with_corrupted_length_prefix() {
        let (mut client_end, mut server_end) = duplex(1024);

        // Send only 2 bytes instead of 4 for length
        tokio
            ::spawn(async move {
                let _ = client_end.write_all(&[0x00, 0x01]).await;
            }).await
            .unwrap();

        let result = receive_data(&mut server_end).await;
        assert!(result.is_err(), "Expected an error due to corrupted length prefix");
    }

    #[tokio::test]
    async fn test_receive_data_with_incomplete_payload() {
        let (mut client_end, mut server_end) = duplex(1024);
        let data = b"hello world";

        tokio
            ::spawn(async move {
                let data_len = (data.len() as u32).to_be_bytes();
                let _ = client_end.write_all(&data_len).await;
                let _ = client_end.write_all(&data[..5]).await; // Send only part of the data
                drop(client_end); // Close the stream early
            }).await
            .unwrap();

        let result = receive_data(&mut server_end).await;
        assert!(result.is_err(), "Expected an error due to incomplete payload");
    }

    #[tokio::test]
    async fn test_receive_json_success() -> std::io::Result<()> {
        use serde::{ Deserialize, Serialize };

        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct MyData {
            name: String,
            age: u8,
        }

        let payload = MyData {
            name: "Alice".to_string(),
            age: 30,
        };

        let serialized = serde_json::to_vec(&payload).unwrap();
        let (mut client_end, mut server_end) = duplex(1024);

        tokio::spawn(async move {
            send_data(&mut client_end, &serialized).await.unwrap();
        });

        let result: MyData = receive_json(&mut server_end).await?;
        assert_eq!(result, payload);
        Ok(())
    }

    #[tokio::test]
    async fn test_receive_json_invalid_format() -> std::io::Result<()> {
        let (mut client_end, mut server_end) = duplex(1024);

        let garbage_data = b"definitely not json";

        tokio::spawn(async move {
            send_data(&mut client_end, garbage_data).await.unwrap();
        });

        let result: std::io::Result<serde_json::Value> = receive_json(&mut server_end).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
        Ok(())
    }

    #[tokio::test]
    async fn test_receive_json_truncated_payload() -> std::io::Result<()> {
        let full_json = br#"{"key": "value"}"#;
        let truncated_json = &full_json[..5]; // Cut off mid-string

        let (mut client_end, mut server_end) = duplex(1024);
        let length = (full_json.len() as u32).to_be_bytes();

        tokio::spawn(async move {
            client_end.write_all(&length).await.unwrap();
            client_end.write_all(truncated_json).await.unwrap(); // only part of data
            drop(client_end); // simulate disconnect
        });

        let result: std::io::Result<serde_json::Value> = receive_json(&mut server_end).await;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_receive_json_type_mismatch() -> std::io::Result<()> {
        use tokio::io::duplex;
        use serde::Deserialize;

        #[derive(Debug, Deserialize)]
        struct ExpectedStruct {
            _id: u32, // we are not using this id
        }

        let wrong_json = serde_json::json!({ "unexpected_key": "oops" });
        let serialized = serde_json::to_vec(&wrong_json).unwrap();
        let (mut client_end, mut server_end) = duplex(1024);

        tokio::spawn(async move {
            send_data(&mut client_end, &serialized).await.unwrap();
        });

        let result: std::io::Result<ExpectedStruct> = receive_json(&mut server_end).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
        Ok(())
    }
}
