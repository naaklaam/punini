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
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Padding, Paragraph, Wrap},
};
use ratatui_image::{
    picker::Picker,
    protocol::StatefulProtocol,
    Resize, StatefulImage,
};
use regex::Regex;
use rodio::{Decoder, OutputStream, Sink, Source};
use std::fs::{self, File};
use std::io::{self, stdout, BufReader, Cursor};
use std::path::Path;
use std::time::Duration;

// --- KONFIGURASI FILE (GANTI SESUAI PATH KAMU) ---
const FILE_PATH: &str = "/home/naaklaam/Flac/Moonlight.flac";

// Struktur data untuk satu baris lirik
#[derive(Clone)]
struct LyricLine {
    time: Duration,
    text: String,
}

struct AppState {
    // Metadata
    title: String,
    artist: String,
    album: String,
    duration: Duration,
    
    // Audio Engine
    sink: Sink,
    _stream: OutputStream, // Perlu disimpan agar audio tidak drop
    
    // Visuals
    cover_art: Option<Box<dyn StatefulProtocol>>,
    
    // Lyrics System
    lyrics: Vec<LyricLine>, 
    lyrics_state: ListState, 
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
    
    // Default Values
    let mut title = "Unknown Title".to_string();
    let mut artist = "Unknown Artist".to_string();
    let mut album = "Unknown Album".to_string();
    let mut cover_protocol = None;
    let mut lyrics_data = Vec::new();

    if let Some(t) = tag {
        title = t.title().as_deref().unwrap_or("Unknown").to_string();
        artist = t.artist().as_deref().unwrap_or("Unknown").to_string();
        album = t.album().as_deref().unwrap_or("Unknown").to_string();

        // A. Load Cover Art (Kitty Protocol)
        if let Some(pic) = t.pictures().first() {
            // Gunakan Result::ok() untuk mengubah error menjadi Option None agar app tidak crash
            if let Ok(mut picker) = Picker::from_termios() {
                 let img_reader = ImageReader::new(Cursor::new(pic.data()))
                    .with_guessed_format()?;
                if let Ok(decoded_img) = img_reader.decode() {
                    cover_protocol = Some(picker.new_resize_protocol(decoded_img));
                }
            }
        }

        // B. Load Lyrics (.lrc file priority)
        let lrc_path = path.with_extension("lrc");
        if lrc_path.exists() {
            if let Ok(content) = fs::read_to_string(lrc_path) {
                lyrics_data = parse_lrc(&content);
            }
        } else {
            // C. Fallback: Embedded Lyrics
            for item in t.items() {
                 if item.key() == &lofty::tag::ItemKey::Lyrics {
                     if let lofty::tag::ItemValue::Text(text) = item.value() {
                         lyrics_data = parse_lrc(text);
                         break;
                     }
                 }
            }
        }
    }

    // 3. Play Audio
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let source = Decoder::new(reader)?;
    let total_duration = source.total_duration().unwrap_or(Duration::from_secs(0));
    
    sink.append(source);
    sink.play();

    // 4. Init State
    let mut app = AppState {
        title,
        artist,
        album,
        duration: total_duration,
        sink,
        _stream,
        cover_art: cover_protocol,
        lyrics: lyrics_data,
        lyrics_state: ListState::default(),
    };

    // 5. Setup Terminal UI
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 6. Run App Loop
    let res = run_app(&mut terminal, &mut app);

    // 7. Cleanup (PENTING)
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

        // --- Logic Sinkronisasi Lirik ---
        let current_pos = app.sink.get_pos();
        
        if !app.lyrics.is_empty() {
            // Cari index lirik terakhir yang waktunya <= waktu lagu sekarang
            let active_idx = app.lyrics.iter()
                .rposition(|line| line.time <= current_pos);
            
            // Update state list agar auto-scroll
            app.lyrics_state.select(active_idx);
        }
        // --------------------------------

        // Event Poll (Update UI setiap 100ms)
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char(' ') => {
                            if app.sink.is_paused() { app.sink.play(); } 
                            else { app.sink.pause(); }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn ui(f: &mut Frame, app: &mut AppState) {
    // Layout Utama: Vertikal (Body & Footer)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(f.area());

    // Layout Body: Horizontal (Kiri: Art, Kanan: Info & Lirik)
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[0]);

    // Layout Kanan: Vertikal (Atas: Info, Bawah: Lirik)
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(0)])
        .split(body_chunks[1]);

    // --- 1. COVER ART ---
    let block_cover = Block::default()
        .borders(Borders::ALL)
        .title(" Art ")
        .border_style(Style::default().fg(Color::Cyan));
    
    // Hitung area dalam border agar gambar rapi
    let cover_area = block_cover.inner(body_chunks[0]);
    f.render_widget(block_cover, body_chunks[0]);

    if let Some(protocol) = &mut app.cover_art {
        let image = StatefulImage::new(None).resize(Resize::Fit(None));
        f.render_stateful_widget(image, cover_area, protocol);
    } else {
        f.render_widget(
            Paragraph::new("No Image").alignment(Alignment::Center),
            cover_area,
        );
    }

    // --- 2. METADATA ---
    let info_text = vec![
        Line::from(vec![Span::raw("Title : "), Span::styled(&app.title, Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow))]),
        Line::from(vec![Span::raw("Artist: "), Span::styled(&app.artist, Style::default().add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::raw("Album : "), Span::styled(&app.album, Style::default().fg(Color::Gray))]),
    ];
    let block_info = Block::default()
        .borders(Borders::ALL)
        .title(" Info ")
        .padding(Padding::new(1,1,1,1));
    
    f.render_widget(Paragraph::new(info_text).block(block_info), right_chunks[0]);

    // --- 3. LYRICS ---
    let block_lyrics = Block::default()
        .borders(Borders::ALL)
        .title(" Lyrics ");
    
    if app.lyrics.is_empty() {
        f.render_widget(
            Paragraph::new("No lyrics found.").block(block_lyrics).alignment(Alignment::Center),
            right_chunks[1]
        );
    } else {
        let items: Vec<ListItem> = app.lyrics.iter().map(|line| {
            // Format waktu [mm:ss]
            let time_str = format!("[{:02}:{:02}] ", line.time.as_secs()/60, line.time.as_secs()%60);
            
            ListItem::new(Line::from(vec![
                Span::styled(time_str, Style::default().fg(Color::DarkGray)),
                Span::raw(&line.text),
            ]))
        }).collect();

        // Style Highlight (Lirik Aktif)
        let lyrics_list = List::new(items)
            .block(block_lyrics)
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD))
            .highlight_symbol(">> ");

        f.render_stateful_widget(lyrics_list, right_chunks[1], &mut app.lyrics_state);
    }

    // --- 4. PROGRESS BAR ---
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
        .block(Block::default().borders(Borders::ALL))
        .gauge_style(Style::default().fg(Color::Magenta))
        .ratio(ratio)
        .label(label);

    f.render_widget(gauge, chunks[1]);
}

// --- FUNGSI PARSING BARU DARI USER ---
fn parse_lrc(content: &str) -> Vec<LyricLine> {
    // Regex yang lebih fleksibel
    // Menangkap [00:00.00] atau [00:00]
    let re = Regex::new(r"\[(\d{2}):(\d{2})(?:\.(\d{2,3}))?\](.*)").unwrap();
    let mut lines = Vec::new();

    for line in content.lines() {
        let line_trimmed = line.trim();
        
        // Skip metadata tags (ar, ti, al, etc) dan baris kosong
        if line_trimmed.is_empty() || !re.is_match(line_trimmed) {
            continue;
        }

        if let Some(cap) = re.captures(line_trimmed) {
            // Parse Menit
            let min: u64 = cap[1].parse().unwrap_or(0);
            
            // Parse Detik
            let sec: u64 = cap[2].parse().unwrap_or(0);
            
            // Parse Milidetik (opsional)
            let millis: u64 = if let Some(m) = cap.get(3) {
                let m_str = m.as_str();
                // Normalisasi: .10 -> 100ms, .5 -> 500ms, .123 -> 123ms
                match m_str.len() {
                    1 => m_str.parse::<u64>().unwrap_or(0) * 100,
                    2 => m_str.parse::<u64>().unwrap_or(0) * 10,
                    _ => m_str.parse::<u64>().unwrap_or(0),
                }
            } else {
                0
            };

            let text = cap[4].trim().to_string();
            
            // Jika teks kosong (hanya timestamp), tetap masukkan tapi sebagai spacer
            // Tapi untuk UI yang bersih, bisa kita skip kalau mau.
            // Di sini kita masukkan saja.
            
            let total_duration = Duration::from_secs(min * 60 + sec) + Duration::from_millis(millis);

            lines.push(LyricLine { time: total_duration, text });
        }
    }
    
    // Urutkan berdasarkan waktu (penting!)
    lines.sort_by_key(|k| k.time);
    lines
}