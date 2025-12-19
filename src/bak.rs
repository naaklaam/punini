use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use image::ImageReader;
use lofty::prelude::*;
use lofty::probe::Probe;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Gauge, Paragraph, Padding, Wrap}, // <--- FIXED: Import Padding
};
use ratatui_image::{
    picker::Picker,
    protocol::StatefulProtocol,
    Resize, StatefulImage,
};
use rodio::{Decoder, OutputStream, Sink, Source}; // <--- FIXED: Import Source trait
use std::fs::File;
use std::io::{self, stdout, BufReader, Cursor};
use std::path::Path;
use std::time::Duration;

// --- GANTI INI DENGAN FILE ASLI KAMU ---
const FILE_PATH: &str = "/home/naaklaam/Flac/Moonlight.flac";

struct AppState {
    title: String,
    artist: String,
    album: String,
    duration: Duration,
    sink: Sink,
    _stream: OutputStream,
    cover_art: Option<Box<dyn StatefulProtocol>>,
}

fn main() -> Result<()> {
    // 1. Setup Audio Device
    let (_stream, stream_handle) = OutputStream::try_default().context("No audio device")?;
    let sink = Sink::try_new(&stream_handle).context("Failed to create sink")?;

    // 2. Load File & Metadata
    let path = Path::new(FILE_PATH);
    if !path.exists() {
        eprintln!("File not found: {}", FILE_PATH);
        return Ok(());
    }

    let tagged_file = Probe::open(path)?.read()?;
    let tag = tagged_file.primary_tag();

    let mut title = "Unknown Title".to_string();
    let mut artist = "Unknown Artist".to_string();
    let mut album = "Unknown Album".to_string();
    let mut cover_protocol = None;

    if let Some(t) = tag {
        title = t.title().as_deref().unwrap_or("Unknown").to_string();
        artist = t.artist().as_deref().unwrap_or("Unknown").to_string();
        album = t.album().as_deref().unwrap_or("Unknown").to_string();

        if let Some(pic) = t.pictures().first() {
            // FIXED: Handling error Picker dengan map_err agar kompatibel dengan anyhow
            // Picker::from_termios bisa gagal kalau dijalankan bukan di terminal asli (misal pipe)
            if let Ok(mut picker) = Picker::from_termios() {
                 let img_reader = ImageReader::new(Cursor::new(pic.data()))
                    .with_guessed_format()?;

                if let Ok(decoded_img) = img_reader.decode() {
                    let protocol = picker.new_resize_protocol(decoded_img);
                    cover_protocol = Some(protocol);
                }
            }
        }
    }

    // 3. Start Playback
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let source = Decoder::new(reader)?;
    // FIXED: Source trait sudah diimport, total_duration() bisa dipakai
    let total_duration = source.total_duration().unwrap_or(Duration::from_secs(0));

    sink.append(source);
    sink.play();

    // 4. Inisialisasi App State
    let mut app = AppState {
        title,
        artist,
        album,
        duration: total_duration,
        sink,
        _stream,
        cover_art: cover_protocol,
    };

    // 5. Setup Terminal UI
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 6. Run Loop
    let res = run_app(&mut terminal, &mut app);

    // 7. Cleanup
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut AppState) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char(' ') => {
                            if app.sink.is_paused() {
                                app.sink.play();
                            } else {
                                app.sink.pause();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn ui(f: &mut Frame, app: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(f.area());

    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[0]);

    // --- WIDGET 1: COVER ART (KIRI) ---
    // Render Block (Border) dulu
    let block_cover = Block::default()
        .borders(Borders::ALL)
        .title(" Cover Art ")
        .border_style(Style::default().fg(Color::Cyan));

    // Ambil area di dalam border agar gambar tidak menimpa garis border
    let cover_area = block_cover.inner(body_chunks[0]);

    // Render Border kosongnya
    f.render_widget(block_cover, body_chunks[0]);

    if let Some(protocol) = &mut app.cover_art {
        // Render Gambarnya DI DALAM area border (cover_area)
        // FIXED: Hapus .block() karena StatefulImage tidak punya method itu
        let image = StatefulImage::new(None).resize(Resize::Fit(None));
        f.render_stateful_widget(image, cover_area, protocol);
    } else {
        f.render_widget(
            Paragraph::new("No Image Data").alignment(Alignment::Center),
            cover_area, // Render teks di dalam area border
        );
    }

    // --- WIDGET 2: METADATA (KANAN) ---
    let info_text = vec![
        Line::from(vec![Span::raw("Title : "), Span::styled(&app.title, Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow))]),
        Line::from(""),
        Line::from(vec![Span::raw("Artist: "), Span::styled(&app.artist, Style::default().add_modifier(Modifier::BOLD))]),
        Line::from(""),
        Line::from(vec![Span::raw("Album : "), Span::styled(&app.album, Style::default().fg(Color::Gray))]),
    ];

    let block_info = Block::default()
        .borders(Borders::ALL)
        .title(" Metadata ")
        .padding(Padding::new(2, 2, 2, 2)); // FIXED: Padding sudah diimport

    f.render_widget(Paragraph::new(info_text).block(block_info).wrap(Wrap { trim: true }), body_chunks[1]);

    // --- WIDGET 3: PROGRESS BAR (BAWAH) ---
    let current_pos = app.sink.get_pos();
    let total_secs = app.duration.as_secs_f64();
    let current_secs = current_pos.as_secs_f64();

    let ratio = if total_secs > 0.0 {
        (current_secs / total_secs).min(1.0)
    } else {
        0.0
    };

    let label = format!(
        "{:02}:{:02} / {:02}:{:02}",
        current_secs as u64 / 60, current_secs as u64 % 60,
        total_secs as u64 / 60, total_secs as u64 % 60
    );

    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" Now Playing "))
        .gauge_style(Style::default().fg(Color::Magenta))
        .ratio(ratio)
        .label(label);

    f.render_widget(gauge, chunks[1]);
}
