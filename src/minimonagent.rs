use bytesize::ByteSize;
use httparse;
use minijinja::{context, Environment};
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::{fmt, cmp};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{
    io::prelude::*,
    net::{TcpListener, TcpStream},
};
use sysinfo::{Disks, System};

const KEEP: usize = 500; // keep this many measurement in RAM. Per mountpoint.
const MEASURE_DELAY: u64 = 60; // capture every MEASURE_DELAY seconds new measurements

fn get_now() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(n) => n.as_secs(),
        Err(_) => 0, // panic!("no time"),
    }
}

#[derive(Debug, Serialize, Clone, PartialEq)]
struct DiskMeasurement {
    ts: u64, // seconds since epoch
    bytes_total: u64,
    bytes_free: u64,
}

impl fmt::Display for DiskMeasurement {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let pct = 100.0 * self.bytes_free as f32 / self.bytes_total as f32;
        write!(
            f,
            "<DiskMeasurement<{0} total: {1} free: {2} {pct:.2}>",
            self.ts, self.bytes_total, self.bytes_free
        )
    }
}

fn consolidate_similar(v : &mut VecDeque<DiskMeasurement>) -> bool {
    let mut to_be_removed_idxs = Vec::<usize>::new();

    if v.len() >= 2 {
        let mut prev = v.front().unwrap();
        // if the disk size changed, keep the last of the old and the first of the new size
        for i in 1..v.len()-1 {
            let cur = v.get(i).unwrap();
            if prev.bytes_total == cur.bytes_total {
                let min_diff_in_bytes = cmp::max(1024, prev.bytes_total / 10000); // 0.01% but at least 1k
                let diff = if prev.bytes_free > cur.bytes_free {
                    prev.bytes_free - cur.bytes_free
                } else {
                    cur.bytes_free - prev.bytes_free
                };
                if diff <= min_diff_in_bytes {
                    to_be_removed_idxs.push(i);
                } else {
                    prev = cur;
                }
            }
        }
    }
    to_be_removed_idxs.sort();
    to_be_removed_idxs.reverse();
    for idx in to_be_removed_idxs.iter() {
        v.remove(*idx);
    }
    to_be_removed_idxs.len() > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_consolidate_similar() {
        let mut v = VecDeque::<DiskMeasurement>::new();
        assert!(!consolidate_similar(&mut v)); // chech that nothing changes for the empty vec
        v.push_back(DiskMeasurement { ts:0, bytes_total: 100_000, bytes_free: 80_000});
        assert!(!consolidate_similar(&mut v)); // chech that nothing changes for the vec with 1
        // element
        v.push_back(DiskMeasurement { ts:10, bytes_total: 100_000, bytes_free: 80_005});
        v.push_back(DiskMeasurement { ts:20, bytes_total: 100_000, bytes_free: 78_000});
        v.push_back(DiskMeasurement { ts:30, bytes_total: 100_000, bytes_free: 78_100});
        v.push_back(DiskMeasurement { ts:40, bytes_total: 100_000, bytes_free: 78_100});
        let v0 = v.clone();
        assert!(consolidate_similar(&mut v));
        assert_eq!(v0[0], v[0]);
        assert_eq!([0, 20, 40], *v.into_iter().filter_map(|e| Some(e.ts)).collect::<Vec<_>>());
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
                // min relevant difference is 0.01% but at least 1k
                if q.len() >= KEEP {
                    consolidate_similar(q );
                    // keep the size of the deque at / below a max.
                    while q.len() >= KEEP {
                        q.pop_front();
                    }
                }
                q.push_back(dm);
            }
            None => {}
        }
    }
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
                    tup.1.len(),
                ),
            )
        }));

        //dbg!(&rows);

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
        // dbg!(&contents);
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
    //dbg!(&req);

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

fn remove_old_mountpoints(
    measurements: &Arc<Mutex<HashMap<String, VecDeque<DiskMeasurement>>>>,
    measurements_older_than: u64,
) {
    let m = &mut *measurements.lock().unwrap();

    m.retain(|_k, v| match v.into_iter().last() {
        Some(dm) => {
            dm.ts > measurements_older_than
        }
        None => false,
    });
}

fn measure_thread(measurements: &Arc<Mutex<HashMap<String, VecDeque<DiskMeasurement>>>>) {
    let mut measurements = measurements.clone();
    thread::spawn(move || loop {
        loop {
            let now = get_now();
            read_diskspace(now, &mut measurements);
            remove_old_mountpoints(&mut measurements, now - 24*60*60); // remove if no new
            // measurements for a day

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

    let mut env = Environment::new();
    env.add_template("home", include_str!("../templates/home.jinja"))
        .unwrap();

    let measurements = Arc::new(Mutex::new(
        HashMap::<String, VecDeque<DiskMeasurement>>::new(),
    ));

    measure_thread(&measurements);
    httpserver_thread(&listener, &env, &measurements, &hostname);
}
