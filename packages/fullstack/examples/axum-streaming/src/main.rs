use dioxus::prelude::*;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use server_fn::codec::{JsonStream, StreamingJson};

fn app() -> Element {
    let mut response = use_signal(String::new);

    rsx! {
        button {
            onclick: move |_| async move {
                response.write().clear();
                if let Ok(stream) = test_stream().await {
                    tracing::info!("Stream started");
                    response.write().push_str("Stream started\n");
                    let mut stream = stream.into_inner();
                    while let Some(Ok(text)) = stream.next().await {
                        tracing::info!("{text:?}");
                        let num = text.number * text.squared;
                        let text = text.text;
                        response.write().push_str(&format!("{num}: {text}\n"));
                    }
                }
            },
            "Start stream"
        }
        "{response}"
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Message {
    number: u64,
    squared: u64,
    text: String,
}

#[server(output = StreamingJson)]
pub async fn test_stream() -> Result<JsonStream<Message>, ServerFnError> {
    let (tx, rx) = futures::channel::mpsc::unbounded();
    tokio::spawn(async move {
        let mut number = 0;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            println!("Sending message {}", number);
            let _ = tx.unbounded_send(Ok(Message {
                number,
                squared: number * number,
                text: "Hello, world!".to_string(),
            }));
            number += 1;
        }
    });

    Ok(JsonStream::new(rx))
}

fn main() {
    #[cfg(target_arch = "wasm32")]
    tracing_wasm::set_as_global_default();

    launch(app)
}
