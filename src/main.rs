use std::env;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::thread;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal;
use log::*;
use minifb::{Key, Window, WindowOptions, Scale};
use padme_core::{FRAME_HEIGHT, FRAME_WIDTH, Button, Rom, System, Pixel, Screen, SerialOutput, AudioSpeaker, AUDIO_SAMPLE_RATE};

fn play_frame<T: cpal::Sample>(outbuffer: &mut[T], sample_buf: &Arc<Mutex<Vec<f32>>>) {
    let mut sample_buf = sample_buf.lock().unwrap();
    let min = std::cmp::min(outbuffer.len(), sample_buf.len());

    for (i, s) in sample_buf.drain(..min).enumerate() {
        outbuffer[i] = cpal::Sample::from(&s);
    }
}

fn create_stream(sample_buf: &Arc<Mutex<Vec<f32>>>) -> cpal::Stream {
    let host = cpal::default_host();
    let device = host.default_output_device().unwrap();
    let sample_rate = cpal::SampleRate(AUDIO_SAMPLE_RATE);
    let mut supported_configs = device.supported_output_configs().unwrap();
    // Find a config that supports:
    // - stereo
    // - float 32
    // - sample rate = 48kHz
    let supported_config = supported_configs.find(| cnf | cnf.channels() == 2
                                                  && sample_rate >= cnf.min_sample_rate()
                                                  && sample_rate <= cnf.max_sample_rate()
                                                  && cnf.sample_format() == cpal::SampleFormat::F32).unwrap();
    let supported_config = supported_config.with_sample_rate(sample_rate);
    let sample_buf = sample_buf.clone();
    let stream = device.build_output_stream(
        &supported_config.config(),
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| play_frame(data, &sample_buf),
        |err| error!("error while playing audio: {}", err),
    ).unwrap();

    stream.play().unwrap();

    stream
}

pub struct AudioPlayer {
    /// A dynamic buffer of samples
    sample_buf: Arc<Mutex<Vec<f32>>>,
    /// Keep the stream alive
    pub stream: cpal::Stream,
}

impl AudioPlayer {
    pub fn new() -> Self {
        let sample_buf = Arc::new(Mutex::new(Vec::new()));
        let stream = create_stream(&sample_buf);

        Self {
            sample_buf,
            stream,
        }
    }
}

impl AudioSpeaker for AudioPlayer {
    fn set_samples(&mut self, left: f32, right: f32) {
        let mut sample_buf = self.sample_buf.lock().unwrap();
        let max_len = ((AUDIO_SAMPLE_RATE * 300) / 1000) as usize;
        // stop if the buffer has more than 300ms of samples
        if sample_buf.len() < max_len {
            sample_buf.push(left);
            sample_buf.push(right);
        }
    }
}

pub struct Lcd {
    framebuffer: [u32; FRAME_WIDTH * FRAME_HEIGHT],
    pub win: Window,
}

impl Lcd {
    pub fn new(title: String) -> Self {
        let win = Window::new(
            &title,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            WindowOptions {
                scale: Scale::X4,
                ..WindowOptions::default()
            },
        ).unwrap();

        Self {
            framebuffer: [0xFFFFFFFFu32; FRAME_WIDTH * FRAME_HEIGHT],
            win,
        }
    }
}

impl Screen for Lcd {
    fn set_pixel(&mut self, px: &Pixel, x: u8, y: u8) {
        let i = x as usize + y as usize * FRAME_WIDTH;
        self.framebuffer[i] = px.rgb();
    }

    fn update(&mut self) {
        self.win
            .update_with_buffer(&self.framebuffer, FRAME_WIDTH, FRAME_HEIGHT)
            .unwrap();
    }
}

pub struct SerialConsole {
    file: File,
}

impl SerialConsole {
    pub fn new(filename: &str) -> Self {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(filename)
            .unwrap();

        Self { file }
    }
}

impl SerialOutput for SerialConsole {
    fn putchar(&mut self, c: u8) {
        self.file.write(&[c]).unwrap();
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    env_logger::builder()
        .format_timestamp(None)
        .init();
    let f: Vec<u8> = std::fs::read(&args[1]).unwrap();

    let rom = Rom::load(f).unwrap();

    info!("{:?}", rom);

    let title = rom.title().unwrap_or(&String::default()).to_owned();
    let mut emu = System::new(rom,
                              Lcd::new(title),
                              SerialConsole::new("/tmp/padme_serial.log"),
                              AudioPlayer::new());

    emu.set_frame_rate(60);

    while emu.screen().win.is_open() && !emu.screen().win.is_key_down(Key::Escape) {
        let t0 = Instant::now();

        emu.update_frame();

        let a_pressed = emu.screen().win.is_key_down(Key::A);
        let b_pressed = emu.screen().win.is_key_down(Key::S);
        let start_pressed = emu.screen().win.is_key_down(Key::Enter);
        let select_pressed = emu.screen().win.is_key_down(Key::Tab);
        let up_pressed = emu.screen().win.is_key_down(Key::Up);
        let down_pressed = emu.screen().win.is_key_down(Key::Down);
        let left_pressed = emu.screen().win.is_key_down(Key::Left);
        let right_pressed = emu.screen().win.is_key_down(Key::Right);

        emu.set_button(Button::A, a_pressed);
        emu.set_button(Button::B, b_pressed);
        emu.set_button(Button::Start, start_pressed);
        emu.set_button(Button::Select, select_pressed);
        emu.set_button(Button::Up, up_pressed);
        emu.set_button(Button::Down, down_pressed);
        emu.set_button(Button::Left, left_pressed);
        emu.set_button(Button::Right, right_pressed);


        let frame_time = t0.elapsed();
        let min_frame_time = emu.min_frame_time();

        if frame_time < min_frame_time {
            thread::sleep(min_frame_time - frame_time);
        }
    }
}
