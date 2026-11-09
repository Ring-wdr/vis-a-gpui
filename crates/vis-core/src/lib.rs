//! vis-a-lot(브라우저 안의 "서버": PGlite + Render Worker)를 Rust로 옮긴 데이터·집계·렌더 엔진.
//! UI는 없다. GPUI 앱(vis-app)과 벤치마크(vis-bench)가 함께 쓴다.

pub mod analytics;
pub mod bench;
pub mod csv_import;
pub mod dataset;
pub mod generator;
pub mod history;
pub mod job;
pub mod options;
pub mod query;
pub mod random;
pub mod region;
pub mod render;
pub mod spec;
pub mod store;
pub mod timefmt;
