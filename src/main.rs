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
use std::path::{Path, PathBuf};
use std::time::Duration;

// --- KONFIGURASI FOLDER MUSIK ---
const MUSIC_DIR: &str = "/home/naaklaam/Music";

// Struktur data lirik
#[derive(Clone)]
struct LyricLine {
    time: Duration,
    text: String,
}

struct AppState {
    // --- Player System ---
    sink: Sink,
    _stream: OutputStream,

    // --- Track Metadata ---
    title: String,
    artist: String,
    album: String,
    duration: Duration,
    cover_art: Option<Box<dyn StatefulProtocol>>,

    // --- Lyrics System ---
    lyrics: Vec<LyricLine>,
    lyrics_state: ListState,

    // --- File Browser System ---
    files: Vec<PathBuf>,      // Daftar file audio yang ditemukan
    file_list_state: ListState, // Posisi kursor di daftar file
}

impl AppState {
    // Fungsi untuk memuat lagu baru ke dalam state
    fn load_track(&mut self, path: &Path) {
        // 1. Stop track sebelumnya (jika ada)
        if !self.sink.empty() {
            self.sink.stop();
            // Buat sink baru karena rodio sink kadang bug kalau distop paksa lalu diappend lagi
            // Tapi untuk simplisitas, kita coba stop & append dulu.
            // Jika ada isu tumpuk suara, kita harus recreate sink (agak kompleks di sini).
            // Solusi paling aman di rodio sederhana: sleep sebentar atau biarkan sink mengelola queue.
            // Di sini kita pakai cara "Append baru". Rodio akan mainkan antrian.
            // Karena kita mau "Ganti Lagu", kita harus clear queue. Sayangnya Rodio v0.19
            // sink.clear() belum stabil. Kita mainkan logika sederhana:
            // Sink akan terus kita pakai.
        }

        // Reset Metadata Visual
        self.title = "Loading...".to_string();
        self.artist = "-".to_string();
        self.album = "-".to_string();
        self.cover_art = None;
        self.lyrics = vec![];
        self.duration = Duration::from_secs(0);

        // 2. Baca Audio File
        let file_res = File::open(path);
        if let Ok(file) = file_res {
            let reader = BufReader::new(file);
            if let Ok(source) = Decoder::new(reader) {
                self.duration = source.total_duration().unwrap_or(Duration::from_secs(0));

                // Hack untuk Rodio: Buat Sink baru setiap ganti lagu adalah cara paling aman
                // untuk menghindari suara menumpuk, tapi sink butuh stream_handle.
                // Disini kita pakai `sink.append` tapi sebelumnya kita `sink.stop()`.
                // Perilaku `stop` rodio adalah mengosongkan queue.
                self.sink.stop();
                self.sink.append(source);
                self.sink.play();
            }
        }

        // 3. Baca Metadata (Lofty)
        if let Ok(tagged_file) = Probe::open(path).and_then(|p| p.read()) {
            if let Some(t) = tagged_file.primary_tag() {
                self.title = t.title().as_deref().unwrap_or("Unknown Title").to_string();
                self.artist = t.artist().as_deref().unwrap_or("Unknown Artist").to_string();
                self.album = t.album().as_deref().unwrap_or("Unknown Album").to_string();

                // Cover Art
                if let Some(pic) = t.pictures().first() {
                    if let Ok(mut picker) = Picker::from_termios() {
                         let img_reader = ImageReader::new(Cursor::new(pic.data()));
                         if let Ok(reader) = img_reader.with_guessed_format() {
                             if let Ok(decoded) = reader.decode() {
                                 self.cover_art = Some(picker.new_resize_protocol(decoded));
                             }
                         }
                    }
                }

                // Lyrics
                let lrc_path = path.with_extension("lrc");
                if lrc_path.exists() {
                     if let Ok(content) = fs::read_to_string(lrc_path) {
                         self.lyrics = parse_lrc(&content);
                     }
                } else {
                    // Embedded Lyrics check
                    for item in t.items() {
                        if item.key() == &lofty::tag::ItemKey::Lyrics {
                            if let lofty::tag::ItemValue::Text(text) = item.value() {
                                self.lyrics = parse_lrc(text);
                                break;
                            }
                        }
                    }
                }
            }
        } else {
            // Jika gagal baca tag, pakai nama file
            self.title = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        }
    }
}

fn main() -> Result<()> {
    // 1. Setup Audio
    let (_stream, stream_handle) = OutputStream::try_default().context("No audio device")?;
    let sink = Sink::try_new(&stream_handle).context("Failed to create sink")?;

    // 2. Scan Folder Musik
    let music_path = Path::new(MUSIC_DIR);
    let mut files = Vec::new();
    if music_path.exists() {
        if let Ok(entries) = fs::read_dir(music_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        let ext_str = ext.to_string_lossy().to_lowercase();
                        if ["flac", "mp3", "wav", "ogg", "m4a"].contains(&ext_str.as_str()) {
                            files.push(path);
                        }
                    }
                }
            }
        }
    }
    // Urutkan file berdasarkan nama
    files.sort();

    // 3. Init State (Kosong dulu)
    let mut app = AppState {
        sink,
        _stream,
        title: "No Track Playing".to_string(),
        artist: "".to_string(),
        album: "".to_string(),
        duration: Duration::from_secs(0),
        cover_art: None,
        lyrics: vec![],
        lyrics_state: ListState::default(),

        files,
        file_list_state: ListState::default(),
    };

    // Pilih file pertama secara default (tapi belum di-load/play)
    if !app.files.is_empty() {
        app.file_list_state.select(Some(0));
    }

    // 4. UI Loop
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, &mut app);

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
            let active_idx = app.lyrics.iter().rposition(|line| line.time <= current_pos);
            app.lyrics_state.select(active_idx);
        }

        // --- Event Handling ---
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),

                        // Play / Pause
                        KeyCode::Char(' ') => {
                            if app.sink.is_paused() { app.sink.play(); }
                            else { app.sink.pause(); }
                        }

                        // Navigasi File (Atas/Bawah/j/k)
                        KeyCode::Up | KeyCode::Char('k') => {
                            let i = match app.file_list_state.selected() {
                                Some(i) => if i == 0 { app.files.len() - 1 } else { i - 1 },
                                None => 0,
                            };
                            app.file_list_state.select(Some(i));
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let i = match app.file_list_state.selected() {
                                Some(i) => if i >= app.files.len() - 1 { 0 } else { i + 1 },
                                None => 0,
                            };
                            app.file_list_state.select(Some(i));
                        }

                        // Play Selected File (Enter)
                        KeyCode::Enter => {
                            if let Some(i) = app.file_list_state.selected() {
                                if let Some(path) = app.files.get(i) {
                                    // Cloning path karena load_track butuh &Path dan app dipinjam mut
                                    let path_clone = path.clone();
                                    app.load_track(&path_clone);
                                }
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
    // 1. Layout Utama: Kiri (Files 30%) - Kanan (Player 70%)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(f.area());

    // --- PANEL KIRI: FILE LIST ---
    let files_block = Block::default().borders(Borders::ALL).title(" Playlist (Music Folder) ");

    let items: Vec<ListItem> = app.files.iter().map(|path| {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        // Cek apakah ini file yang sedang diputar? (Optional visual hint)
        // Disini kita render biasa saja
        ListItem::new(name).style(Style::default())
    }).collect();

    let list = List::new(items)
        .block(files_block)
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");

    f.render_stateful_widget(list, main_chunks[0], &mut app.file_list_state);

    // --- PANEL KANAN: PLAYER ---
    // Bagi panel kanan: Vertikal (Body & Progress)
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(main_chunks[1]);

    // Bagi Body: Kiri (Cover) - Kanan (Meta & Lyrics)
    let player_body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(right_chunks[0]);

    // Bagian Kanan (Meta & Lyrics)
    let meta_lyrics = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(0)])
        .split(player_body[1]);

    // 1. Cover Art
    let block_cover = Block::default().borders(Borders::ALL).title(" Art ").fg(Color::Cyan);
    let cover_area = block_cover.inner(player_body[0]);
    f.render_widget(block_cover, player_body[0]);

    if let Some(protocol) = &mut app.cover_art {
        let image = StatefulImage::new(None).resize(Resize::Fit(None));
        f.render_stateful_widget(image, cover_area, protocol);
    }

    // 2. Metadata
    let info_text = vec![
        Line::from(vec![Span::raw("Title : "), Span::styled(&app.title, Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow))]),
        Line::from(vec![Span::raw("Artist: "), Span::styled(&app.artist, Style::default().add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::raw("Album : "), Span::styled(&app.album, Style::default().fg(Color::Gray))]),
    ];
    let block_info = Block::default().borders(Borders::ALL).title(" Info ").padding(Padding::new(1,1,1,1));
    f.render_widget(Paragraph::new(info_text).block(block_info), meta_lyrics[0]);

    // 3. Lyrics
    let block_lyrics = Block::default().borders(Borders::ALL).title(" Lyrics ");
    if app.lyrics.is_empty() {
        f.render_widget(Paragraph::new("No lyrics.").block(block_lyrics).alignment(Alignment::Center), meta_lyrics[1]);
    } else {
        let items: Vec<ListItem> = app.lyrics.iter().map(|line| {
            let time_str = format!("[{:02}:{:02}] ", line.time.as_secs()/60, line.time.as_secs()%60);
            ListItem::new(Line::from(vec![
                Span::styled(time_str, Style::default().fg(Color::DarkGray)),
                Span::raw(&line.text),
            ]))
        }).collect();

        let lyrics_list = List::new(items)
            .block(block_lyrics)
            .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD))
            .highlight_symbol(">> ");

        f.render_stateful_widget(lyrics_list, meta_lyrics[1], &mut app.lyrics_state);
    }

    // 4. Progress Bar
    let current_pos = app.sink.get_pos();
    let total_secs = app.duration.as_secs_f64();
    let current_secs = current_pos.as_secs_f64();
    let ratio = if total_secs > 0.0 { (current_secs / total_secs).min(1.0) } else { 0.0 };
    let label = format!("{:02}:{:02} / {:02}:{:02}", current_secs as u64/60, current_secs as u64%60, total_secs as u64/60, total_secs as u64%60);

    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL))
        .gauge_style(Style::default().fg(Color::Magenta))
        .ratio(ratio)
        .label(label);
    f.render_widget(gauge, right_chunks[1]);
}

fn parse_lrc(content: &str) -> Vec<LyricLine> {
    let re = Regex::new(r"\[(\d{2}):(\d{2})(?:\.(\d{2,3}))?\](.*)").unwrap();
    let mut lines = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || !re.is_match(line) { continue; }
        if let Some(cap) = re.captures(line) {
            let min: u64 = cap[1].parse().unwrap_or(0);
            let sec: u64 = cap[2].parse().unwrap_or(0);
            let millis: u64 = if let Some(m) = cap.get(3) {
                let m_str = m.as_str();
                match m_str.len() {
                    1 => m_str.parse::<u64>().unwrap_or(0) * 100,
                    2 => m_str.parse::<u64>().unwrap_or(0) * 10,
                    _ => m_str.parse::<u64>().unwrap_or(0),
                }
            } else { 0 };
            let time = Duration::from_secs(min * 60 + sec) + Duration::from_millis(millis);
            lines.push(LyricLine { time, text: cap[4].trim().to_string() });
        }
    }
    lines.sort_by_key(|k| k.time);
    lines
}
