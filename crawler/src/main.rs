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

#[derive(Serialize, Deserialize, Debug)]
struct RestaurantInfo {
    code: usize,
    name: String,
    course: String,
    url: String,
    tn_url: String,
    address: String,
    business_hours: Vec<BussinessHours>,
    business_hours_raw: String,
    regular_holiday: Vec<String>,
    regular_holiday_raw: String,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
struct BussinessHours {
    day_of_week: Option<String>,
    open: Time,
    close: Time,
    lo: Option<Time>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, Ord, PartialOrd, Eq, PartialEq)]
struct Time {
    hour: usize,
    min: usize,
}

impl Time {
    fn new(hour: usize, min: usize) -> Self {
        Self { hour, min }
    }
}

fn parse_business_hours_line(s: &str) -> Option<Vec<BussinessHours>> {
    // 正規化
    let s = s.replace("～", "〜");
    let s = s.replace("（", "(");
    let s = s.replace("）", ")");
    let s = s.replace("L.O.", "LO");
    let s = s.replace("L.O", "LO");
    let s = s.replace("ＬＯ", "LO");
    let s = s.replace("平日", "月〜金");
    let s = s.replace("月曜", "月");
    let s = s.replace("土曜日", "土");
    let s = s.replace("土曜", "土");
    let s = s.replace("から", "〜");

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

    let s = Regex::new(r"([月火水木金土日祝]+) *(ランチ|ディナー|カフェ|バー|モーニング|ティー)")
        .unwrap()
        .replace_all(&s, "$2 $1");

    let s = Regex::new(r"、$").unwrap().replace_all(&s, "");

    let s = s.replace("：", "");

    let s = s.trim();

    let re = Regex::new(
        r#"^(?P<type>カフェ|ランチ|ディナー|バー|モーニング|ティー)? *(?P<day>[月火水木金土日祝]+)? *(?P<open_hour>[0-9]+):(?P<open_min>[0-9]+)〜(?P<close_hour>[0-9]+):(?P<close_min>[0-9]+) *(\((?P<lo>LO) *((?P<lo_hour>[0-9]+):(?P<lo_min>[0-9]+)?)?\))?$"#,
    )
    .unwrap();

    let ms = re.captures(s);

    if ms.is_none() {
        dbg!(s);
        return None;
    }

    let ms = ms.unwrap();

    let get_int = |n: &str| -> usize { ms.name(n).unwrap().as_str().parse().unwrap() };

    let mut open = Time::new(get_int("open_hour"), get_int("open_min"));
    let mut close = Time::new(get_int("close_hour"), get_int("close_min"));
    let mut lo = if let (Some(h), Some(m)) = (ms.name("lo_hour"), ms.name("lo_min")) {
        Some(Time::new(
            h.as_str().parse().unwrap(),
            m.as_str().parse().unwrap(),
        ))
    } else if ms.name("lo").is_some() {
        Some(close.clone())
    } else {
        None
    };

    if close <= open {
        close.hour += 24;
    }

    if let Some(lo) = lo.as_mut() {
        if lo < &mut open {
            lo.hour += 24;
        }
    }

    let days = ms.name("day");

    let bh = BussinessHours {
        day_of_week: None,
        open,
        close,
        lo,
    };

    Some(if let Some(days) = days {
        days.as_str()
            .chars()
            .map(|c| BussinessHours {
                day_of_week: Some(c.to_string()),
                ..bh.clone()
            })
            .collect()
    } else {
        vec![bh]
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
        if let Some(mut t) = t {
            ret.append(&mut t);
        } else {
            dbg!(&s);
            break;
        }
    }

    ret
}

fn parse_regular_holiday(s: &str) -> Vec<String> {
    // 正規化
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

    let s = s.replace("月曜日", "月");
    let s = s.replace("火曜日", "火");
    let s = s.replace("水曜日", "水");
    let s = s.replace("土曜日", "土");
    let s = s.replace("日曜日", "日");
    let s = s.replace("日曜", "日");
    let s = s.replace("祝日", "祝");
    let s = s.replace("なし", "年中無休");
    let s = s.replace("年中無休", "無休");
    let s = s.replace("・", "");
    let s = s.replace("、", "");
    let s = s.replace("ディナー", "");

    // タイポ？
    let s = s.replace("年始年始", "年末年始");

    let s = s.trim();

    let re = Regex::new(r#"^(([月火水木金土日祝]|年末年始|無休|不定休))+$"#).unwrap();

    let caps = re.captures(s);

    if caps.is_none() {
        panic!("{:?}", s);
    }

    let re = Regex::new(
        r#"^(?P<day>[月火水木金土日祝]+)|(?P<ny>年末年始)|(?P<none>無休)|(?P<unst>不定休)"#,
    )
    .unwrap();

    let mut ret = vec![];
    let mut none = false;

    for caps in re.captures_iter(s) {
        if caps.name("none").is_some() {
            none = true;
            continue;
        }

        if let Some(day) = caps.name("day") {
            for c in day.as_str().chars() {
                ret.push(c.to_string());
            }
            continue;
        }

        if caps.name("ny").is_some() {
            ret.push("年末年始".to_string());
            continue;
        }
    }

    assert!(if none { ret.is_empty() } else { true });

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
    let infos: Vec<RawInfo> = serde_json::from_reader(File::open("sanitized.json")?)?;
    let raw_infos: Vec<RawInfo> = serde_json::from_reader(File::open("raw.json")?)?;

    let mut parsed = vec![];

    for info in infos {
        let raw = raw_infos.iter().find(|r| r.name == info.name).unwrap();

        let bh = parse_business_hours(&info.business_hours);
        let raw_bh = raw.business_hours.clone();

        let rh = parse_regular_holiday(&info.regular_holiday);
        let raw_rh = raw.regular_holiday.clone();

        let dat = RestaurantInfo {
            code: info.code,
            name: info.name.clone(),
            course: info.course.clone(),
            url: info.url.clone(),
            tn_url: info.tn_url.clone(),
            address: info.address.clone(),
            business_hours: bh,
            business_hours_raw: raw_bh,
            regular_holiday: rh,
            regular_holiday_raw: raw_rh,
        };

        parsed.push(dat);
    }

    fs::write("info.json", serde_json::to_string(&parsed)?)?;

    Ok(())
}

#[cmd_group(verbose, commands = [get_index, get_data, parse])]
fn main() -> Result<()> {}
