use axum::{
    extract::{Query, State},
    http::{
        header::{HeaderMap, ACCEPT},
        status::StatusCode,
    },
    response::{Html, IntoResponse, Response},
    Json,
};
use bytesize::ByteSize;
use minijinja::render;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{cmp, fmt};
use sysinfo::{Disk, Disks};

const KEEP: usize = 500; // keep this many measurement in RAM. Per mountpoint.
const MEASURE_DELAY: u64 = 6; // capture every MEASURE_DELAY seconds new measurements

fn get_now() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(n) => n.as_secs(),
        Err(_) => 0, // panic!("no time"),
    }
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct DiskMeasurement {
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

#[derive(Debug, Clone)]
pub struct AppState {
    pub measurements: Arc<Mutex<HashMap<String, VecDeque<DiskMeasurement>>>>,
    pub hostname: String,
}

fn consolidate_similar(v: &mut VecDeque<DiskMeasurement>) -> bool {
    let mut to_be_removed_idxs = Vec::<usize>::new();

    if v.len() >= 2 {
        let mut prev = v.front().unwrap();
        // if the disk size changed, keep the last of the old and the first of the new size
        for i in 1..v.len() - 1 {
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
mod test_consolidate_similar {
    use super::*;

    #[test]
    fn test_consolidate_similar() {
        let mut v = VecDeque::<DiskMeasurement>::new();
        assert!(!consolidate_similar(&mut v)); // chech that nothing changes for the empty vec
        v.push_back(DiskMeasurement {
            ts: 0,
            bytes_total: 100_000,
            bytes_free: 80_000,
        });
        assert!(!consolidate_similar(&mut v)); // chech that nothing changes for the vec with 1
                                               // element
        v.push_back(DiskMeasurement {
            ts: 10,
            bytes_total: 100_000,
            bytes_free: 80_005,
        });
        v.push_back(DiskMeasurement {
            ts: 20,
            bytes_total: 100_000,
            bytes_free: 78_000,
        });
        v.push_back(DiskMeasurement {
            ts: 30,
            bytes_total: 100_000,
            bytes_free: 78_100,
        });
        v.push_back(DiskMeasurement {
            ts: 40,
            bytes_total: 100_000,
            bytes_free: 78_100,
        });
        let v0 = v.clone();
        assert!(consolidate_similar(&mut v));
        assert_eq!(v0[0], v[0]);
        assert_eq!(
            [0, 20, 40],
            *v.into_iter().filter_map(|e| Some(e.ts)).collect::<Vec<_>>()
        );
    }
}

fn read_diskspace(now: u64, disk: &Disk) -> (String, DiskMeasurement) {
    let mnt = disk.mount_point();
    let mnt_name: String = mnt.display().to_string();
    let bytes_total = disk.total_space();
    let bytes_free = disk.available_space();
    let dm = DiskMeasurement {
        ts: now,
        bytes_total,
        bytes_free,
    };
    (mnt_name, dm)
}

fn add_diskmeasurement(
    m: &mut HashMap<String, VecDeque<DiskMeasurement>>,
    mnt_name: String,
    dm: DiskMeasurement,
) {
    // mountpoints come and go
    if !m.contains_key(&mnt_name) {
        m.insert(mnt_name.clone(), VecDeque::<DiskMeasurement>::new());
    }
    match m.get_mut(&mnt_name) {
        Some(q) => {
            // min relevant difference is 0.01% but at least 1k
            if q.len() >= KEEP {
                consolidate_similar(q);
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

pub fn read_diskspaces(now: u64, m: &mut HashMap<String, VecDeque<DiskMeasurement>>) {
    for disk in &Disks::new_with_refreshed_list() {
        let (mnt_name, dm) = read_diskspace(now, disk);
        add_diskmeasurement(m, mnt_name, dm);
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

fn match_str(input: &[u8], pat: &str) -> bool {
    let s = match std::str::from_utf8(input) {
        Ok(v) => v,
        Err(_) => "",
    };
    s.contains(pat)
}

#[derive(Deserialize, Debug)]
pub struct HomeParams {
    #[serde(default)]
    since: u64,
}

pub fn create_filtered_copy_dms(
    orig: &HashMap<String, VecDeque<DiskMeasurement>>,
    threshold: u64,
) -> HashMap<String, VecDeque<DiskMeasurement>> {
    let mut c = HashMap::<String, VecDeque<DiskMeasurement>>::new();
    for (k, v) in orig.iter() {
        let mut v = v.clone();
        v.retain(|dm| dm.ts >= threshold);
        c.insert(k.clone(), v);
    }
    c
}
#[cfg(test)]
mod test_create_filtered_copy_dms {
    use super::*;

    fn gen_testdata(dms: &mut HashMap<String, VecDeque<DiskMeasurement>>, mp: &String) {
        if !dms.contains_key(mp) {
            dms.insert(mp.clone(), VecDeque::<DiskMeasurement>::new());
        }

        dms.insert(mp.clone(), VecDeque::<DiskMeasurement>::new());
        let v = dms.get_mut(mp).unwrap();
        for i in (0..100).step_by(10) {
            v.push_back(DiskMeasurement {
                ts: i,
                bytes_total: 100_000,
                bytes_free: 80_000 - (i * 200),
            });
        }
    }
    #[test]
    fn test_create_filtered_copy_dms() {
        let mut dms = HashMap::<String, VecDeque<DiskMeasurement>>::new();
        let mp = String::from("my_only_mp");
        gen_testdata(&mut dms, &mp);
        assert_eq!(1, dms.len());

        // cut nothing away
        let fdms = create_filtered_copy_dms(&dms, 0);
        assert_eq!(10, fdms.get(&mp).expect("full").len());
        assert_eq!(0, fdms.get(&mp).expect("full").front().unwrap().ts);
        assert_eq!(90, fdms.get(&mp).expect("full").iter().last().unwrap().ts);

        // cut the first away
        let fdms = create_filtered_copy_dms(&dms, 10);
        assert_eq!(9, fdms.get(&mp).expect("full").len());
        assert_eq!(10, fdms.get(&mp).expect("full").front().unwrap().ts);
        assert_eq!(90, fdms.get(&mp).expect("full").iter().last().unwrap().ts);

        // cut all but the last away
        let fdms = create_filtered_copy_dms(&dms, 90);
        assert_eq!(1, fdms.get(&mp).expect("full").len());
        assert_eq!(90, fdms.get(&mp).expect("full").front().unwrap().ts);
        assert_eq!(90, fdms.get(&mp).expect("full").iter().last().unwrap().ts);
        //
        // cut all but the last away
        let fdms = create_filtered_copy_dms(&dms, 91);
        assert_eq!(0, fdms.get(&mp).expect("full").len());
    }
}

pub async fn home(
    State(state): State<AppState>,
    headers: HeaderMap,
    params: Query<HomeParams>,
) -> Response {
    //dbg!(&params);
    //dbg!(&headers);
    let accept_header = headers.get(ACCEPT);

    match accept_header {
        Some(ac) if match_str(ac.as_bytes(), "text/html") => {
            let hm = &state.measurements.lock().unwrap();
            let rows = Vec::from_iter(hm.iter().map(|tup| {
                (
                    tup.0, // mount point
                    tup.1 // list of measurements
                        .into_iter()
                        .filter(|dm| dm.ts >= params.since)
                        .map(|dm| convert_disk_measurement(dm))
                        .collect::<Vec<_>>(),
                    (
                        ByteSize(tup.1.into_iter().last().unwrap().bytes_total).to_string(),
                        ByteSize(tup.1.into_iter().last().unwrap().bytes_free).to_string(),
                        tup.1.len(),
                    ),
                )
            }));

            let tmpl = include_str!("../templates/home.jinja");
            let contents = render!(tmpl,
                    hostname => state.hostname,
                    rows => rows,
            );
            Html(contents).into_response()
        }
        Some(ac) if match_str(ac.as_bytes(), "application/json") => {
            let hm = &state.measurements.lock().unwrap();
            if params.since > 0 {
                // create a copy and cut off the older measurements
                let c = create_filtered_copy_dms(&hm, params.since);
                Json(json!(c)).into_response()
            } else {
                Json(json!(**hm)).into_response()
            }
        }

        _ => StatusCode::BAD_REQUEST.into_response(),
    }
}

fn remove_old_mountpoints(
    m: &mut HashMap<String, VecDeque<DiskMeasurement>>,
    measurements_older_than: u64,
) {
    //let m = &mut *measurements.lock().unwrap();

    m.retain(|_k, v| match v.into_iter().last() {
        Some(dm) => dm.ts > measurements_older_than,
        None => false,
    });
}

pub fn measure_disk_thread(measurements: Arc<Mutex<HashMap<String, VecDeque<DiskMeasurement>>>>) {
    thread::spawn(move || loop {
        loop {
            let now = get_now();
            {
                let m = &mut measurements.lock().unwrap();
                read_diskspaces(now, m);
                remove_old_mountpoints(m, now - 24 * 60 * 60); // remove if no new measurements for a day
            }

            thread::sleep(Duration::from_secs(MEASURE_DELAY));
        }
    });
}
