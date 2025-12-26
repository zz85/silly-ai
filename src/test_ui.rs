//! Test UI rendering without audio

use crate::render::Ui;
use crate::tui::Tui;
use std::error::Error;
use std::time::Duration;
use tokio::time::sleep;

pub async fn run(scene: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    let (ui, ui_rx) = Ui::new();
    let mut tui = Tui::new()?;

    // Tick task for animations
    let ui_tick = ui.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            ui_tick.tick();
        }
    });

    match scene {
        "interactive" => run_interactive(&ui, &mut tui, &ui_rx).await?,
        "preview" => {
            for i in 1..=3 {
                ui.set_preview(format!("Transcribing{}...", ".".repeat(i)));
                process_events(&mut tui, &ui_rx)?;
                tui.draw()?;
                sleep(Duration::from_millis(500)).await;
            }
            ui.set_preview("Hello this is a preview".into());
            process_events(&mut tui, &ui_rx)?;
            tui.draw()?;
            sleep(Duration::from_secs(2)).await;
        }
        "thinking" => {
            ui.set_thinking();
            for _ in 0..30 {
                process_events(&mut tui, &ui_rx)?;
                tui.draw()?;
                sleep(Duration::from_millis(100)).await;
            }
        }
        "speaking" => {
            ui.set_speaking();
            for _ in 0..30 {
                process_events(&mut tui, &ui_rx)?;
                tui.draw()?;
                sleep(Duration::from_millis(100)).await;
            }
        }
        "response" => {
            ui.set_thinking();
            for _ in 0..10 {
                process_events(&mut tui, &ui_rx)?;
                tui.draw()?;
                sleep(Duration::from_millis(100)).await;
            }
            for word in "Hello! I am Silly, your AI assistant.".split_whitespace() {
                ui.append_response(&format!("{} ", word));
                process_events(&mut tui, &ui_rx)?;
                tui.draw()?;
                sleep(Duration::from_millis(100)).await;
            }
            ui.end_response();
            process_events(&mut tui, &ui_rx)?;
            tui.draw()?;
            sleep(Duration::from_millis(500)).await;
        }
        _ => {
            // Run all scenes
            ui.set_preview("Hello this is preview text".into());
            for _ in 0..20 {
                process_events(&mut tui, &ui_rx)?;
                tui.draw()?;
                sleep(Duration::from_millis(100)).await;
            }

            ui.show_final("Final transcription");
            process_events(&mut tui, &ui_rx)?;
            tui.draw()?;
            sleep(Duration::from_millis(500)).await;

            ui.set_thinking();
            for _ in 0..20 {
                process_events(&mut tui, &ui_rx)?;
                tui.draw()?;
                sleep(Duration::from_millis(100)).await;
            }

            for word in "Hello! I am Silly.".split_whitespace() {
                ui.append_response(&format!("{} ", word));
                process_events(&mut tui, &ui_rx)?;
                tui.draw()?;
                sleep(Duration::from_millis(150)).await;
            }
            ui.end_response();
            process_events(&mut tui, &ui_rx)?;
            tui.draw()?;

            ui.set_speaking();
            for _ in 0..20 {
                process_events(&mut tui, &ui_rx)?;
                tui.draw()?;
                sleep(Duration::from_millis(100)).await;
            }
        }
    }

    ui.set_idle();
    process_events(&mut tui, &ui_rx)?;
    tui.draw()?;
    Ok(())
}

fn process_events(tui: &mut Tui, rx: &flume::Receiver<crate::render::UiEvent>) -> std::io::Result<()> {
    while let Ok(event) = rx.try_recv() {
        tui.handle_ui_event(event)?;
    }
    Ok(())
}

async fn run_interactive(
    ui: &Ui,
    tui: &mut Tui,
    ui_rx: &flume::Receiver<crate::render::UiEvent>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut words = 100usize;
    ui.set_context_words(words);

    loop {
        process_events(tui, ui_rx)?;
        tui.draw()?;

        if let Some(input) = tui.poll_input()? {
            if input == "\x03" {
                break;
            }
            match input.as_str() {
                "p" => ui.set_preview("Preview text...".into()),
                "t" => ui.set_thinking(),
                "s" => ui.set_speaking(),
                "r" => {
                    for word in "Streaming response text.".split_whitespace() {
                        ui.append_response(&format!("{} ", word));
                        process_events(tui, ui_rx)?;
                        tui.draw()?;
                        sleep(Duration::from_millis(100)).await;
                    }
                    ui.end_response();
                }
                "f" => ui.show_final("Final transcription"),
                "i" => ui.set_idle(),
                "+" => {
                    words = words.saturating_add(50);
                    ui.set_context_words(words);
                }
                "-" => {
                    words = words.saturating_sub(50);
                    ui.set_context_words(words);
                }
                "q" => break,
                "help" | "?" => {
                    println!("\nCommands: p=preview t=thinking s=speaking r=response f=final i=idle +/- words q=quit\n");
                }
                _ => {}
            }
        }

        sleep(Duration::from_millis(10)).await;
    }

    Ok(())
}
