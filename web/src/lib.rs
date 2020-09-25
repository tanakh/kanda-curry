#![recursion_limit = "1024"]

use chrono::{Date, DateTime, Datelike, FixedOffset, Timelike, Utc, Weekday};
use log::*;
use serde::{Deserialize, Serialize};
use std::{
    cmp::{max, min},
    collections::BTreeMap,
    sync::RwLock,
};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};
use yew::prelude::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
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

impl BussinessHours {
    fn time_to_close(&self, dt: &DateTime<FixedOffset>) -> usize {
        let date = dt.date();
        let time = dt.time();

        let hour = time.hour();
        let minute = time.minute();

        // 早朝は前日扱いにする
        let (hour, minute, date) = if (hour, minute) < (5, 0) {
            (hour + 24, minute, date.pred())
        } else {
            (hour, minute, date)
        };

        let wd = jp_weekday_name(date.weekday());
        let holiday = is_holiday(&date);

        // info!("{:?}, {:?}", self, dt);

        if let Some(w) = &self.day_of_week {
            if w == "祝" {
                if !holiday {
                    return 0;
                }
            } else {
                if w != wd {
                    return 0;
                }
            }
        }

        let tm = Time::new(hour as _, minute as _);

        if let Some(lo) = &self.lo {
            if self.open <= tm && &tm < lo {
                lo.diff_min(&tm) as _
            } else {
                0
            }
        } else {
            if self.open <= tm && tm < self.close {
                self.close.diff_min(&tm) as _
            } else {
                0
            }
        }
    }
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

    fn to_min(&self) -> usize {
        self.hour * 60 + self.min
    }

    fn diff_min(&self, rhs: &Time) -> isize {
        self.to_min() as isize - rhs.to_min() as isize
    }
}

struct Model {}

impl Component for Model {
    type Message = ();
    type Properties = ();
    fn create(_: Self::Properties, _: ComponentLink<Self>) -> Self {
        Self {}
    }

    fn update(&mut self, _: Self::Message) -> ShouldRender {
        true
    }

    fn change(&mut self, _props: Self::Properties) -> ShouldRender {
        false
    }

    fn view(&self) -> Html {
        html! {
            <>
            <div class="jumbotron text-center">
                <h1>{"神田カレーグランプリ スタンプラリー2020"}</h1>
                <p class="lead">{"🍛営業中店舗検索ツール🍛"}</p>
            </div>

            <div class="container">
                <p>
                    <a href="https://kanda-curry.com/?page_id=12180">{"🍛神田カレーグランプリ スタンプラリー2020🍛"}</a>
                    {" の営業中店舗を検索するツールです。"}
                </p>
                <MainComponent/>
            </div>

            <footer class="footer mt-auto" style="text-align: center; padding: 20px; background-color: #f5f5f5;">
                <div class="container">
                    <span class="text-muted">{"Copyright© 2020 Hideyuki Tanaka"}</span>
                </div>
            </footer>

            </>
        }
    }
}

lazy_static::lazy_static! {
    static ref RESTAURANT_INFO: RwLock<Vec<RestaurantInfo>> = RwLock::new(vec![]);
}

struct MainComponent {
    link: ComponentLink<Self>,
    props: Props,
}

#[derive(Properties, Clone)]
struct Props {
    #[prop_or(get_jst_time())]
    dt: DateTime<FixedOffset>,
    #[prop_or(get_selected_course())]
    selected_courses: Vec<bool>,
    #[prop_or(false)]
    include_visited: bool,
    #[prop_or(get_visited())]
    visited: Vec<bool>,
}

enum Msg {
    DateTime(ChangeData),
    SelectCourse(char),
    Visited(usize),
    IncludeVisited,
}

impl Component for MainComponent {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        MainComponent { link, props }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::DateTime(ChangeData::Value(s)) => {
                let s = format!("{}+0900", s);
                info!("value: {}", s);
                self.props.dt = dbg!(DateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M%z").unwrap());
            }
            Msg::SelectCourse(c) => {
                let ix = "ABCDE".find(c).unwrap();
                self.props.selected_courses[ix] = !self.props.selected_courses[ix];
                set_selected_course(&self.props.selected_courses)
            }
            Msg::Visited(i) => {
                self.props.visited[i] = !self.props.visited[i];
                set_visited(&self.props.visited);
            }
            Msg::IncludeVisited => {
                self.props.include_visited = !self.props.include_visited;
            }
            _ => unreachable!(),
        }
        true
    }

    fn change(&mut self, _props: Self::Properties) -> ShouldRender {
        false
    }

    fn view(&self) -> Html {
        let dt = &self.props.dt;

        let weekday = jp_weekday_name(dt.weekday());
        let holiday = is_holiday(&dt.date());

        // info!("Cur time: {:?}", dt);

        let lock = RESTAURANT_INFO.read().unwrap();

        let visited = lock.iter().enumerate().filter(|(i, r)| {
            let ix = "ABCDE".find(&r.course).unwrap();
            self.props.visited[*i] && self.props.selected_courses[ix]
        });

        let seatch_target_cnt = lock
            .iter()
            .enumerate()
            .filter(|(i, r)| {
                if !self.props.include_visited && self.props.visited[*i] {
                    return false;
                }
                let ix = "ABCDE".find(&r.course).unwrap();
                self.props.selected_courses[ix]
            })
            .count();

        let (avails, not_avails): (Vec<_>, Vec<_>) = lock
            .iter()
            .enumerate()
            .filter(|(i, r)| {
                if !self.props.include_visited && self.props.visited[*i] {
                    return false;
                }
                let ix = "ABCDE".find(&r.course).unwrap();
                self.props.selected_courses[ix]
            })
            .map(|(i, r)| {
                let is_closed = r
                    .regular_holiday
                    .iter()
                    .any(|r| r == weekday || r == "祝" && holiday);

                let time_to_close = if is_closed {
                    0
                } else {
                    let bh = &r.business_hours;
                    bh.iter().map(|bh| bh.time_to_close(&dt)).max().unwrap_or(0)
                };

                (i, r, time_to_close)
            })
            .partition(|(_, _, time_to_close)| *time_to_close > 0);

        let mut status = BTreeMap::<String, (usize, usize)>::new();
        let mut free_course = 0;

        for (i, r) in lock.iter().enumerate() {
            let course = &r.course;
            let e = status.entry(course.clone()).or_default();
            if self.props.visited[i] {
                e.0 += 1;
                free_course += 1;
            }
            e.1 += 1;
        }

        let cleard_course = max(
            status.iter().filter(|(_, (a, b))| a == b).count(),
            if free_course >= 25 { 1 } else { 0 },
        );

        let degree = match cleard_course {
            0 => "未獲得🥺",
            1 => "神田カレーマイスター🏅",
            2 => "神田カレーブロンズマイスター🥉",
            3 => "神田カレーシルバーマイスター🥈",
            4 => "神田カレーゴールドマイスター🥇",
            5 => "神田カレーグランドマイスター👑",
            _ => unreachable!(),
        };

        let card = |i: usize, r: &RestaurantInfo, time_to_close: usize| {
            let s = r.business_hours_raw.replace("<br>", "\n");
            let bh = s
                .lines()
                .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "));
            let rh = r.regular_holiday_raw.replace("<br>", "\n");

            let address = r.address.replace("<br>", "\n");

            let header_color = match r.course.as_str() {
                "A" => "#e5407e",
                "B" => "#0e80d0",
                "C" => "#df7600",
                "D" => "#50a639",
                "E" => "#7d51a0",
                _ => unreachable!(),
            };

            html! {
                <div class="card">
                    <div class="card-header text-white" style=format!("background: {}", header_color)>
                        <strong>{ format!("{}コース", r.course) }</strong>
                        <div class="float-right">
                            <small>{"訪問済み"}</small>
                            <input type="checkbox" class="ml-2" checked=self.props.visited[i]
                                onclick=self.link.callback(move |_| Msg::Visited(i))/>
                        </div>
                    </div>

                    <a href=r.url.clone()>
                        <img class="bd-placeholder-img card-img-top" width="100%" src=r.tn_url.clone()/>
                    </a>

                    <div class="card-body">
                        <h5 class="card-title">
                            <a href=r.url.clone() class="text-dark">{ htmlescape::decode_html(&r.name).unwrap() }</a>
                        </h5>
                        <p class="card-text">
                        <a href=format!("https://maps.google.co.jp/maps/search/{}",
                            urlencoding::encode(&format!("{} {}", address, r.name)))
                            class="text-secondary">{ address }</a>
                        </p>
                    </div>

                    <ul class="list-group list-group-flush">
                        {
                            if time_to_close > 0 && time_to_close <= 30 {
                                html! {
                                    <li class="list-group-item text-white bg-danger">
                                        { "まもなく営業終了" }
                                    </li>
                                }
                            } else {
                                html! {}
                            }
                        }

                        <li class="list-group-item">
                        { "営業時間：" }
                        {
                            for bh.map(|s| html!{ <><br/>{s}</> })
                        }
                        </li>
                        <li class="list-group-item">
                        { "定休日：" }
                        { rh }
                        </li>
                    </ul>
                </div>
            }
        };

        html! {
            <>

            <h2>{"コース制覇状況"}</h2>
            <br/>

            <p class="h4">{ format!("称号：{}", degree) }</p>
            <br/>

            <table class="d-flex table">
            <tbody>
            {
                for status.iter().map(|(course, (vis, tot))| {
                    html! {
                    <tr>
                        <th scope="row">{ format!("{}コース", course) }</th>
                        <td>{ format!("{} / {}", vis, tot) }</td>
                        <td>{ if vis >= tot {"制覇！"} else {"未制覇"} }</td>
                    </tr>
                    }
                })
            }

            <tr>
                <th scope="row">{ "フリーコース" }</th>
                <td>{ format!("{} / 25", min(25, free_course)) }</td>
                <td>{ if free_course >= 25 {"制覇！"} else {"未制覇"} }</td>
            </tr>

            </tbody>
            </table>

            <hr/>

            <h2>{"検索条件"}</h2>
            <br/>

            <form>
                <div class="form-group row">
                    <label for="dt" class="col-sm-2 col-form-label">{"日時"}</label>
                    <div class="col-sm-4">
                        <input type="datetime-local" id="dt" class="form-control"
                            onchange=self.link.callback(|ev| Msg::DateTime(ev))
                            value=format!(
                            "{}-{:02}-{:02}T{:02}:{:02}",
                            self.props.dt.date().year(),
                            self.props.dt.date().month(),
                            self.props.dt.date().day(),
                            self.props.dt.time().hour(),
                            self.props.dt.time().minute(),
                        )/>
                    </div>
                </div>
                <div class="form-group row">
                    <label class="col-sm-2 col-form-label">{"コース"}</label>
                    {
                        for "ABCDE".chars().enumerate().map(|(i, c)| {
                            let id = format!("checkbox-{}", c);
                            let checked = self.props.selected_courses[i];
                            html!{
                            <div class="form-check form-check-inline">
                                <input class="form-check-input" type="checkbox" id=id checked=checked
                                    onclick=self.link.callback(move |_| Msg::SelectCourse(c)) />
                                <label class="form-check-label" for=id>{format!("{}コース", c)}</label>
                            </div>
                            }
                        })
                    }
                </div>
                <div class="form-group row">
                    <label class="col-sm-2 col-form-label">{"オプション"}</label>
                    <div class="form-check form-check-inline">
                        <input class="form-check-input" type="checkbox" id="include-visited"
                            checked=self.props.include_visited
                            onclick=self.link.callback(|_| Msg::IncludeVisited) />
                        <label for="include-visited" class="form-check-label">{"訪問済み店舗を含める"}</label>
                    </div>
                </div>
            </form>

            <hr/>

            <h2>{ format!("営業中の店舗 ({}/{})", avails.len(), seatch_target_cnt) }</h2>
            <br/>

            <div class="card-columns">
            { for avails.into_iter().map(|r| card(r.0, r.1, r.2)) }
            </div>

            <hr/>

            <h2>{ format!("営業時間外の店舗 ({}/{})", not_avails.len(), seatch_target_cnt) }</h2>
            <br/>

            <div class="card-columns">
            { for not_avails.into_iter().map(|r| card(r.0, r.1, r.2)) }
            </div>

            <hr/>

            <h2>{"訪問済みの店舗"}</h2>
            <br/>

            <div class="card-columns">
            { for visited.into_iter().map(|r| card(r.0, r.1, 0)) }
            </div>
            </>
        }
    }

    fn rendered(&mut self, _first_render: bool) {}

    fn destroy(&mut self) {}
}

fn get_jst_time() -> DateTime<FixedOffset> {
    let hour = 3600;
    let tz = FixedOffset::east(9 * hour);
    Utc::now().with_timezone(&tz)
}

fn jp_weekday_name(wd: Weekday) -> &'static str {
    match wd {
        Weekday::Mon => "月",
        Weekday::Tue => "火",
        Weekday::Wed => "水",
        Weekday::Thu => "木",
        Weekday::Fri => "金",
        Weekday::Sat => "土",
        Weekday::Sun => "日",
    }
}

const JAPANESE_HOLIDAY: &[(u32, u32)] = &[
    // https://www8.cao.go.jp/chosei/shukujitsu/gaiyou.html
    (1, 1),
    (1, 13),
    (2, 11),
    (2, 23),
    (2, 24),
    (3, 20),
    (4, 29),
    (5, 3),
    (5, 4),
    (5, 5),
    (5, 6),
    (7, 23),
    (7, 24),
    (8, 10),
    (9, 21),
    (9, 22),
    (11, 3),
    (11, 23),
];

fn is_holiday(date: &Date<FixedOffset>) -> bool {
    JAPANESE_HOLIDAY
        .iter()
        .any(|&(m, d)| m == date.month() && d == date.day())
}

fn get_visited() -> Vec<bool> {
    let ls = web_sys::window().unwrap().local_storage().unwrap().unwrap();
    let val = ls.get_item("visited").unwrap();

    if let Some(val) = val {
        val.chars().map(|c| c == '1').collect()
    } else {
        vec![false; RESTAURANT_INFO.read().unwrap().len()]
    }
}

fn set_visited(v: &Vec<bool>) {
    let ls = web_sys::window().unwrap().local_storage().unwrap().unwrap();
    let val = v
        .iter()
        .map(|b| if *b { '1' } else { '0' })
        .collect::<String>();
    ls.set_item("visited", &val).unwrap();
}

fn get_selected_course() -> Vec<bool> {
    let ls = web_sys::window().unwrap().local_storage().unwrap().unwrap();
    let val = ls.get_item("selected-course").unwrap();

    if let Some(val) = val {
        val.chars().map(|c| c == '1').collect()
    } else {
        vec![true; 5]
    }
}

fn set_selected_course(v: &Vec<bool>) {
    let ls = web_sys::window().unwrap().local_storage().unwrap().unwrap();
    let val = v
        .iter()
        .map(|b| if *b { '1' } else { '0' })
        .collect::<String>();
    ls.set_item("selected-course", &val).unwrap();
}

#[wasm_bindgen(start)]
pub async fn run_app() -> Result<(), JsValue> {
    web_logger::init();

    let mut opts = RequestInit::new();
    opts.method("GET");
    opts.mode(RequestMode::Cors);
    let url = "./info.json";
    let req = Request::new_with_str_and_init(&url, &opts).unwrap();
    let window = web_sys::window().unwrap();

    let resp = JsFuture::from(window.fetch_with_request(&req))
        .await
        .unwrap();
    let resp: Response = resp.dyn_into().unwrap();
    let json = JsFuture::from(resp.json().unwrap()).await.unwrap();
    let vals: Vec<RestaurantInfo> = json.into_serde().unwrap();

    {
        let mut r = RESTAURANT_INFO.write().unwrap();
        *r = vals;
    }

    App::<Model>::new().mount_to_body();

    Ok(())
}
