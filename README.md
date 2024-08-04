# minimon

minimon is a minimal monitoring package, focusing on disk stats.

The agent keeps all the collected data just in RAM, writes no logfile and is
just minimal invasive. It runs as a systemd daemon. It capture one measurement
for each mountpoint once / minute and keeps 500 measurements (per mount point)
in RAM. Once it get's to 500 measurements, then it will throw away the
measrements, that are close to the previous once.

It a mounitpoint is gone for 2 hours, then all that measrements are thrown away.

## building

```
$ cargo build
```

build the debian / ubuntu package
```
./build_deb.sh
```

## running

```
cargo run --release
```

the agent will listen on port `9988` and show simple stats like this:
![screenshot](images/demo.png)

You can grab the data as json:
```
$ curl -H "Accept: application/json" http://solar:9988/ | python -m json.tool - 
  % Total    % Received % Xferd  Average Speed   Time    Time     Time  Current
                                 Dload  Upload   Total   Spent    Left  Speed
100   208  100   208    0     0   9398      0 --:--:-- --:--:-- --:--:--  9454
{
    "/": [
        {
            "bytes_free": 3323228160,
            "bytes_total": 8350298112,
            "ts": 1722697823
        },
        {
            "bytes_free": 3323252736,
            "bytes_total": 8350298112,
            "ts": 1722698963
        },
        {
            "bytes_free": 3323248640,
            "bytes_total": 8350298112,
            "ts": 1722699023
        }
    ]
}
```

The timestamp (`ts`) is in seconds since the EPOCH. you see a list of measurements. 

