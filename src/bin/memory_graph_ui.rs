#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use std::sync::mpsc;
use std::thread;
use std::net::TcpStream;
use IntelliBoard::ui::memory_graph::MemoryGraphView;
use IntelliBoard::core::ipc_messages::{GraphRequest, GraphResponse};
use IntelliBoard::core::config::load_actions_config;
use std::io::{Write, BufReader};
use std::time::{Duration, Instant};

fn main() -> eframe::Result<()> {
    // Parse args for port
    let args: Vec<String> = std::env::args().collect();
    let port_arg = args.get(1);
    let port = if let Some(p) = port_arg {
         p.parse::<u16>().unwrap_or(12345)
    } else {
        12345
    };
    
    // Load config for export_path
    let export_path = load_actions_config()
        .ok()
        .and_then(|c| c.export_path);

    let stream_res = TcpStream::connect(format!("127.0.0.1:{}", port));
    let stream = match stream_res {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to connect to graph server at port {}: {}", port, e);
            // We can show a dialog or just exit
            return Ok(());
        }
    };
    stream.set_nonblocking(false).ok(); 
    
    let (tx_out, rx_out) = mpsc::channel::<GraphRequest>(); // View (update) -> Writer Thread
    let (tx_in, rx_in) = mpsc::channel::<GraphResponse>(); // Reader Thread -> View (update)
    
    let stream_clone_read = stream.try_clone().unwrap();
    let stream_clone_write = stream.try_clone().unwrap();

    // Reader thread
    thread::spawn(move || {
        let reader = BufReader::new(stream_clone_read);
        let iter = serde_json::Deserializer::from_reader(reader).into_iter::<GraphResponse>();
        for msg in iter {
           if let Ok(msg) = msg {
               if tx_in.send(msg).is_err() { break; }
           } else {
               // Deserialize error or EOF
               break; 
           }
        }
    });

    // Writer thread
    thread::spawn(move || {
        let mut writer = std::io::BufWriter::new(stream_clone_write);
        while let Ok(msg) = rx_out.recv() {
            if serde_json::to_writer(&mut writer, &msg).is_ok() {
                let _ = writer.write(b"\n"); 
                let _ = writer.flush();
            } else {
                break;
            }
        }
    });

    // Initial Snapshot request
    tx_out.send(GraphRequest::GetSnapshot).ok();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 800.0])
            .with_position([900.0, 200.0])
            .with_title("IntelliBoard Memory Graph")
            .with_active(true)
            .with_icon(IntelliBoard::ui::theme::load_egui_icon()),
        ..Default::default()
    };
    
    eframe::run_native(
        "Memory Graph",
        options,
        Box::new(move |cc| {
             IntelliBoard::ui::theme::configure_fonts(&cc.egui_ctx);
             IntelliBoard::ui::theme::apply_theme(&cc.egui_ctx);

             let view = MemoryGraphView::new_with_export_path(export_path.clone());
             Box::new(GraphApp::new(view, rx_in, tx_out))
        }),
    )
}

struct GraphApp {
    view: MemoryGraphView,
    rx_in: mpsc::Receiver<GraphResponse>,
    tx_out: mpsc::Sender<GraphRequest>,

    last_snapshot_request: Instant,
    snapshot_interval: Duration,
    pending_snapshot: bool,
}

impl GraphApp {
    fn new(
        view: MemoryGraphView,
        rx_in: mpsc::Receiver<GraphResponse>,
        tx_out: mpsc::Sender<GraphRequest>,
    ) -> Self {
        Self {
            view,
            rx_in,
            tx_out,
            last_snapshot_request: Instant::now(),
            snapshot_interval: Duration::from_millis(800),
            pending_snapshot: false,
        }
    }

    fn request_snapshot(&mut self) {
        let _ = self.tx_out.send(GraphRequest::GetSnapshot);
        self.last_snapshot_request = Instant::now();
    }
}

impl eframe::App for GraphApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Read incoming responses
        while let Ok(msg) = self.rx_in.try_recv() {
            match msg {
                GraphResponse::Snapshot { items, links } => {
                     self.view.set_data(items, links);
                }
                GraphResponse::DataChanged => {
                    // Server pushed a notification that data changed - request fresh snapshot
                    self.pending_snapshot = true;
                }
                GraphResponse::Ack => {
                    // Some servers may still reply with Ack. Force a refresh soon.
                    self.pending_snapshot = true;
                }
                GraphResponse::Error(e) => {
                    eprintln!("Graph Server Error: {}", e);
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            self.view.draw(ui, ctx);
        });
        
        // Push mutations
        for req in self.view.drain_mutations() {
            self.tx_out.send(req).ok();
            self.pending_snapshot = true;
        }

        // Periodic snapshot poll keeps UI consistent even if we miss an Ack.
        if self.last_snapshot_request.elapsed() >= self.snapshot_interval {
            self.request_snapshot();
        } else if self.pending_snapshot {
            self.pending_snapshot = false;
            self.request_snapshot();
        }

        // Keep the UI responsive while dragging/streaming data.
        ctx.request_repaint_after(Duration::from_millis(50));
        
    }
}
