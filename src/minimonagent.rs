use bytesize::ByteSize;
use httparse;
use minijinja::{context, Environment};
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::mem;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{
    io::prelude::*,
    net::{TcpListener, TcpStream},
};
use sysinfo::{Disks, System};

const KEEP: usize = 100;
const MEASURE_DELAY: u64 = 60; // 60 seconds

#[derive(Debug, Serialize)]
struct DiskMeasurement {
    ts: u64, // seconds since epoch
    bytes_total: u64,
    bytes_free: u64,
}

fn get_now() -> u64 {
    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(n) => n.as_secs(),
        Err(_) => 0, // panic!("no time"),
    };

    now
}

impl fmt::Display for DiskMeasurement {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let pct = 100.0 * self.bytes_free as f32 / self.bytes_total as f32;
        write!(
            f,
            "DiskMeasurement<{0} total: {1} free: {2} {pct:.2}>",
            self.ts, self.bytes_total, self.bytes_free
        )
    }
}

fn read_diskspace(
    now: u64,
    measurements: &mut Arc<Mutex<HashMap<String, VecDeque<DiskMeasurement>>>>,
) {
    for disk in &Disks::new_with_refreshed_list() {
        let mnt = disk.mount_point();
        let mnt_name: String = mnt.display().to_string();
        let bytes_total = disk.total_space();
        let bytes_free = disk.available_space();
        let dm = DiskMeasurement {
            ts: now,
            bytes_total,
            bytes_free,
        };

        // mountpoints come and go
        let mut m = measurements.lock().unwrap();
        if !m.contains_key(&mnt_name) {
            m.insert(mnt_name.clone(), VecDeque::<DiskMeasurement>::new());
        }
        match m.get_mut(&mnt_name) {
            Some(q) => {
                // keep the size of the deque at / below a max.
                while q.len() >= KEEP {
                    q.pop_front();
                }
                q.push_back(dm);
            }
            None => {}
        }
    }
    // TODO: remove the measurements from the hashmap when the mountpoint is gone "long enough",
    // eg 2 hours.
}

fn convert_disk_measurement(dm: &DiskMeasurement) -> (u64, u64, u64, f32) {
    (
        dm.ts,
        dm.bytes_total,
        dm.bytes_free,
        if dm.bytes_total != 0 {
            // percent free
            100.0 * dm.bytes_free as f32 / dm.bytes_total as f32
        } else {
            0.0 // defaults to no free space, aka 0%
        },
    )
}

fn send_home_html(
    mut stream: TcpStream,
    env: &Environment<'_>,
    measurements: &Arc<Mutex<HashMap<String, VecDeque<DiskMeasurement>>>>,
    hostname: &String,
) {
    let status_line = "HTTP/1.1 200 OK";
    let tmpl = env.get_template("home").unwrap();
    let response;
    {
        // keep the mutex lock scope short.
        let hm = &*measurements.lock().unwrap();
        let rows = Vec::from_iter(hm.iter().map(|tup| {
            (
                tup.0, // mount point
                tup.1 // list of measurements
                    .into_iter()
                    .map(|dm| convert_disk_measurement(dm))
                    .collect::<Vec<_>>(),
                (
                    ByteSize(tup.1.into_iter().last().unwrap().bytes_total).to_string(),
                    ByteSize(tup.1.into_iter().last().unwrap().bytes_free).to_string(),
                ),
            )
        }));

        dbg!(&rows);

        let contents = tmpl
            .render(context! {
                hostname => hostname,
                rows => rows,
            })
            .unwrap();

        let length = contents.len();
        response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");
    }

    stream.write_all(response.as_bytes()).unwrap();
}

fn send_home_json(
    mut stream: TcpStream,
    _env: &Environment<'_>,
    measurements: &Arc<Mutex<HashMap<String, VecDeque<DiskMeasurement>>>>,
    _hostname: &String,
) {
    let response;
    {
        let hm = &*measurements.lock().unwrap();
        let contents = json!(hm).to_string();
        dbg!(&contents);
        let length = contents.len();
        let status_line = "HTTP/1.1 200 OK";
        response = format!("{status_line}\r\nContent-Length: {length}\r\nContent-Type: application/json\r\n\r\n{contents}");
    }

    stream.write_all(response.as_bytes()).unwrap();
}

fn handle_connection(
    mut stream: TcpStream,
    env: &Environment<'_>,
    measurements: &Arc<Mutex<HashMap<String, VecDeque<DiskMeasurement>>>>,
    hostname: &String,
) {
    let mut buffer = [0; 10240];
    stream.read(&mut buffer).unwrap();

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    let _res = req.parse(&buffer).unwrap();
    //dbg!(&res);
    dbg!(&req);

    let ah: Vec<_> = req.headers.iter().filter(|h| h.name == "Accept").collect();
    //dbg!(&ah);
    let mut mimetypes: Vec<_> = Vec::<_>::new();
    if ah.len() == 1 {
        let s = match std::str::from_utf8(ah[0].value) {
            Ok(v) => v,
            Err(_) => "",
        };
        if s.len() > 0 {
            mimetypes = s.split(",").collect();
        }
    }

    match req.path {
        Some(path) => {
            if path == "/" {
                if mimetypes.contains(&"application/json") {
                    send_home_json(stream, &env, &measurements, &hostname);
                } else {
                    send_home_html(stream, &env, &measurements, &hostname);
                }
            }
        }
        _ => (),
    }
}

fn measure_thread(measurements: &Arc<Mutex<HashMap<String, VecDeque<DiskMeasurement>>>>) {
    let mut measurements = measurements.clone();
    thread::spawn(move || loop {
        loop {
            let now = get_now();
            read_diskspace(now, &mut measurements);

            thread::sleep(Duration::from_secs(MEASURE_DELAY));
        }
    });
}

fn httpserver_thread(
    listener: &TcpListener,
    env: &Environment<'_>,
    measurements: &Arc<Mutex<HashMap<String, VecDeque<DiskMeasurement>>>>,
    hostname: &String,
) {
    for stream in listener.incoming() {
        let stream = stream.unwrap();

        handle_connection(stream, &env, &measurements, &hostname);
    }
}

fn main() {
    let addr = "0.0.0.0:9988";
    let listener = TcpListener::bind(&addr).unwrap();
    println!("Listening on: {}", addr);

    let hostname = match System::host_name() {
        Some(name) => name,
        None => "horse_with_now_name".to_string(),
    };
    dbg!(mem::size_of::<DiskMeasurement>());
    let mut env = Environment::new();
    env.add_template("home", include_str!("../templates/home.jinja"))
        .unwrap();

    let measurements = Arc::new(Mutex::new(
        HashMap::<String, VecDeque<DiskMeasurement>>::new(),
    ));

    measure_thread(&measurements);
    httpserver_thread(&listener, &env, &measurements, &hostname);
}
