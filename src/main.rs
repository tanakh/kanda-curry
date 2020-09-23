use anyhow::Result;
use argopt::{cmd_group, subcmd};
use easy_scraper::Pattern;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    path::Path,
    thread,
    time::Duration,
};

const STAMP_RARRY_URL: &str = "https://kanda-curry.com/?page_id=12180";

#[derive(Serialize, Deserialize, Debug)]
struct RestaurantIndex {
    name: String,
    course: String,
    url: String,
    tn_url: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct RawInfo {
    code: usize,
    name: String,
    course: String,
    url: String,
    tn_url: String,
    address: String,
    business_hours: String,
    regular_holiday: String,
}

struct RestaurantInfo {
    url: String,
    name: String,
    address: String,
    business_hours: Vec<BussinessHours>,
    // regular_holiday: _,
}

#[derive(Debug, Default)]
struct BussinessHours {
    day_of_week: String,
    open: Time,
    close: Time,
    lo: Option<Time>,
}

#[derive(Debug, Default)]
struct Time {
    hour: usize,
    min: usize,
}

impl Time {
    fn new(hour: usize, min: usize) -> Self {
        Self { hour, min }
    }
}

fn parse_business_hours_line(s: &str) -> Option<BussinessHours> {
    // 月～金 11:00～21:00（LO23：00）
    // 土日祝 11:00～17:00

    let org = s.clone();

    let s = s.trim();

    // 正規化
    let s = s.replace("～", "〜");
    let s = s.replace("（", "(");
    let s = s.replace("）", ")");
    let s = s.replace("L.O.", "LO");
    let s = s.replace("ＬＯ", "LO");
    let s = s.replace("平日", "月〜金");
    let s = s.replace("土曜日", "土");
    let s = s.replace("土曜", "土");
    let s = s.replace("月〜日", "月火水木金土日");
    let s = s.replace("月〜金", "月火水木金");
    let s = s.replace("火〜金", "火水木金");
    let s = s.replace("月〜土", "月火水木金土");

    let s = s.replace("デイナー", "ディナー");

    let s = Regex::new(r"([月火水木金土日祝])・([月火水木金土日祝])")
        .unwrap()
        .replace_all(&s, "$1$2");
    let s = Regex::new(r"([月火水木金土日祝])・([月火水木金土日祝])")
        .unwrap()
        .replace_all(&s, "$1$2");

    let s = Regex::new(r"\(([月火水木金土日祝]+)\)")
        .unwrap()
        .replace_all(&s, "$1");

    let s = Regex::new(r"(\d{2})[：:](\d{2})")
        .unwrap()
        .replace_all(&s, "$1:$2");

    let s = Regex::new(r"([月火水木金土日祝]+) *(ランチ|ディナー|カフェ|バー|モーニング)")
        .unwrap()
        .replace_all(&s, "$2 $1");

    let s = s.replace("：", "");

    let s = &s;

    let re = Regex::new(
        r#"^(?P<type>カフェ|ランチ|ディナー|バー|モーニング)? *(?P<day>[月火水木金土日祝]+)? *(?P<open_hour>[0-9]+):(?P<open_min>[0-9]+)〜(?P<close_hour>[0-9]+):(?P<close_min>[0-9]+) *(\((?P<lo>LO) *((?P<lo_hour>[0-9]+):(?P<lo_min>[0-9]+)?)?\))?$"#,
    )
    .unwrap();

    let ms = re.captures(s);

    if ms.is_none() {
        dbg!(s);
        return None;
    }

    let ms = ms.unwrap();

    let get_int = |n: &str| -> usize { ms.name(n).unwrap().as_str().parse().unwrap() };

    Some(BussinessHours {
        // day_of_week: ms.name("day").,
        day_of_week: "?".to_string(),
        open: Time::new(get_int("open_hour"), get_int("open_min")),
        close: Time::new(get_int("close_hour"), get_int("close_min")),
        lo: if let (Some(h), Some(m)) = (ms.name("lo_hour"), ms.name("lo_min")) {
            Some(Time::new(
                h.as_str().parse().unwrap(),
                m.as_str().parse().unwrap(),
            ))
        } else {
            None
        },
    })
}

fn parse_business_hours(s: &str) -> Vec<BussinessHours> {
    let s = s
        .replace("<br>", "\n")
        .chars()
        .map(|c| {
            if c == '\n' || !c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect::<String>();

    let mut ret = vec![];

    for line in s.lines().filter(|l| !l.is_empty()) {
        if line.starts_with("※") {
            continue;
        }

        if !Regex::new(r"\d[:：]\d").unwrap().is_match(line) {
            continue;
        }

        let t = parse_business_hours_line(line);
        if let Some(t) = t {
            ret.push(t);
        } else {
            dbg!(&s);
            break;
        }
    }

    ret
}

#[subcmd]
fn get_index() -> Result<()> {
    let resp = ureq::get(STAMP_RARRY_URL).call();
    let str = resp.into_string()?;

    let pat = Pattern::new(
        r#"
    <div class="container {{course}}line">
        <div class="card {{course-dup}}course">
            <figure>
                <a href="{{url-dup}}">
                    <img src="{{tn-url}}">
                </a>
            </figure>
            <p class="cardtxt">
                <a href="{{url}}">{{name}}</a>
            </p>
        </div>
    </div>
    "#,
    )
    .unwrap();

    let ms = pat.matches(&str);

    let index = ms
        .into_iter()
        .filter(|r| !r["url"].starts_with("#"))
        .map(|r| RestaurantIndex {
            name: r["name"].to_string(),
            course: r["course"].to_string(),
            url: r["url"].to_string(),
            tn_url: r["tn-url"].to_string(),
        })
        .collect::<Vec<_>>();

    fs::write("index.json", serde_json::to_string(&index)?)?;

    Ok(())
}

#[subcmd]
fn get_data() -> Result<()> {
    let index: Vec<RestaurantIndex> = serde_json::from_reader(File::open("index.json")?)?;

    assert_eq!(index.len(), 100);

    let pat = Pattern::new(
        r#"
    <table class="hyou" subseq>
        <tr><th>店名</th><td>{{name:*}}</td></tr>
        <tr><th>住所</th><td>{{address:*}}</td></tr>
        <tr><th>営業時間</th><td>{{business-hours:*}}</td></tr>
        <tr><th>定休日</th><td>{{regular-holiday:*}}</td></tr>
        <tr><th>カレーグランプリ店舗コード</th><td>{{code}}</td></tr>
    </table>
    "#,
    )
    .unwrap();

    let n = index.len();

    let mut json = vec![];

    for (i, ix) in index.into_iter().enumerate() {
        eprintln!("[{}/{}]: getting info: {}", i + 1, n, ix.url);

        let resp = ureq::get(&ix.url).call();
        if let Some(err) = resp.synthetic_error() {
            eprintln!("failed to get: {}, {:?}", ix.url, err);
            continue;
        }

        let str = resp.into_string()?;

        let ms = pat.matches(&str);
        assert!(ms.len() >= 1);

        let info = &ms[0];

        let info = RawInfo {
            code: info["code"].parse().unwrap(),
            name: info["name"].to_string(),
            course: ix.course.clone(),
            url: ix.url.clone(),
            tn_url: ix.tn_url.clone(),
            address: info["address"].to_string(),
            business_hours: info["business-hours"].to_string(),
            regular_holiday: info["regular-holiday"].to_string(),
        };

        let fname = format!("data/{}.json", info.code);
        if Path::new(&fname).exists() {
            panic!("File {} is already exists", fname);
        }

        eprintln!("saved to {}", &fname);

        fs::write(&fname, serde_json::to_string(&info)?)?;
        json.push(info);
        thread::sleep(Duration::from_millis(1000));
    }

    assert_eq!(json.len(), n);

    fs::write("info.json", serde_json::to_string(&json)?)?;

    Ok(())
}

#[subcmd]
fn parse() -> Result<()> {
    let infos: Vec<RawInfo> = serde_json::from_reader(File::open("info.json")?)?;

    for info in infos {
        let bh = parse_business_hours(&info.business_hours);
        // dbg!(&bh);
    }

    todo!()
}

#[cmd_group(verbose, commands = [get_index, get_data, parse])]
fn main() -> Result<()> {}
