# 성능 비교: vis-a-lot(CSR, 브라우저) vs vis-a-gpui(Rust 네이티브)

질문: **지금 CSR로 도는 vis-a-lot을 Rust 네이티브로 옮기면 클라이언트 쪽 성능이 얼마나 바뀌나?**

짧은 답: 같은 PC, 같은 데이터(같은 시드의 IoT 100만 행), 같은 케이스에서

- **데이터 등록**(생성 + 적재): 브라우저 24.0초 → 네이티브 0.14초 (**약 170배**)
- **차트 데이터 쿼리**(원본 `pnpm bench:db`의 15개 케이스): 중앙값 기준 **17~740배**, 대부분 **100배 이상**
- **draw 한 번 끝까지**(쿼리 + 모든 item 렌더 + PNG): **8~100배**, 흔한 경우 **10~25배**
- 차이가 가장 작은 곳은 **렌더 자체**다. 꺾은선(line)처럼 그리는 양이 많은 차트는 렌더만 따지면 **약 3배**다. 브라우저의 Canvas 2D도 C++ Skia라 원래 빠르다.

즉 큰 이득은 "Rust가 JS보다 빠르다"보다 **구조**에서 나온다: WASM Postgres(PGlite, 단일 스레드) → 열 단위 메모리 배열 + 멀티코어 집계, 결과를 글자로 이어 Worker 사이에 넘기던 비용 → 같은 프로세스의 포인터, PNG 인코딩·Cache API·MSW 왕복 → 비트맵을 바로 GPU 텍스처로.

## 측정 환경과 방법

| 항목 | 값 |
|---|---|
| 기기 | 같은 Linux 컨테이너, Intel Xeon 2.1GHz **4코어**, 16GB, GPU 없음 |
| 웹 | vis-a-lot 0.18.0 (`2953b5a`) 프로덕션 빌드(`pnpm build`, static export). PGlite 0.5.10 (Postgres 18.6 wasm32) |
| 웹 쿼리 | `pnpm bench:db --skip-feed --rows N --iterations 5` (Node 22에서 PGlite 임베디드, 브라우저 DB Worker와 같은 엔진·설정) — warmup 2회 후 5회 중앙값 |
| 웹 draw | `scripts/web-draw-bench.mjs`: 빌드를 headless Chromium(Playwright)에 띄워, 화면과 같은 REST 요청(MSW → Hono 서비스 → DB Worker → Render Worker × 3 → PNG)으로 draw를 끝까지 잼. 시나리오마다 warmup 1회 후 1회 |
| 네이티브 | vis-a-gpui `vis-bench` (release, `opt-level=3`, thin LTO). 쿼리는 warmup 1회 후 5회 중앙값, draw는 warmup 1회 후 3회 중앙값 |
| 렌더 스레드 | 둘 다 3개 (웹: `hardwareConcurrency − 1`, 최대 4 / 네이티브: 코어 수 − 1) |
| item 크기 | 400×300 CSS px, pixelRatio 1, 섬네일(128×80) 포함 |
| 원본 데이터 | `docs/bench/*.json` |

같은 일을 하도록 맞춘 것:

- 데이터: 원본 생성기(`server/data/generator.ts`)를 같은 시드·같은 난수(mulberry32 + Box–Muller)로 옮겨 같은 행을 만든다.
- 쿼리 케이스: 원본 `scripts/bench-db/run.mts`의 이름·조건(필터, split, legend, 통계 함수) 그대로. scatter 케이스는 원본처럼 **split 조합마다 x·y·행 id·legend 배열을 만들 때까지** 잰다.
- draw: 쿼리 → 모든 item 렌더 → 섬네일 → **PNG 인코딩**까지 (`vis-bench --png`). 실제 앱은 PNG를 만들지 않고 비트맵을 바로 화면에 올려 조금 더 빠르다.

## 1. 데이터 등록

| | 웹 | 네이티브 | 배율 |
|---|---:|---:|---:|
| 10만 행: 생성 + 적재 (Node PGlite, `COPY`) | 1.56초 | 10ms + 저장 4ms | 약 110× |
| 100만 행: 생성 + 적재 (Node PGlite) | 12.8초 | 85ms + 저장 53ms | 약 93× |
| 100만 행: 생성 + 적재 (**브라우저**, DB Worker + IndexedDB) | 24.0초 | 85ms + 저장 53ms | 약 170× |
| DB 부팅 (PGlite 생성) | 3.5초 | — (파일을 열면 끝, 100만 행 74ms) | |

## 2. 차트 데이터 쿼리 (원본 `bench:db` 케이스, 중앙값)

### 100만 행

| 케이스 | 웹 PGlite (ms) | 네이티브 (ms) | 배율 |
|---|---:|---:|---:|
| scatter ts×value | 2,246.7 | 13.40 | 168× |
| scatter split factory×line + legend | 2,952.7 | 24.81 | 119× |
| scatter ts 1주 필터 | 905.8 | 2.42 | 375× |
| bar avg value by machine split factory | 345.4 | 10.78 | 32× |
| bar median by line legend sensor | 1,260.7 | 17.06 | 74× |
| boxplot value by sensor_type | 2,133.6 | 17.22 | 124× |
| histogram value split sensor | 596.0 | 12.94 | 46× |
| heatmap ts×machine avg | 1,411.3 | 5.39 | 262× |
| legendValues sensor_type | 246.7 | 0.00 ※ | — |
| columnSummary machine | 224.5 | 0.30 | 744× |
| columnSummary ts | 479.5 | 1.17 | 408× |
| splitCount 3컬럼 | 347.0 | 20.61 | 17× |
| splitCount 필터 | 259.2 | 3.63 | 71× |
| sampleRows 필터 1페이지 | 698.1 | 1.11 | 628× |
| itemRows split 조합 1페이지 | 338.3 | 1.07 | 317× |

### 10만 행

| 케이스 | 웹 PGlite (ms) | 네이티브 (ms) | 배율 |
|---|---:|---:|---:|
| scatter ts×value | 253.6 | 0.73 | 347× |
| scatter split factory×line + legend | 369.1 | 1.69 | 218× |
| scatter ts 1주 필터 | 86.2 | 0.42 | 204× |
| bar avg value by machine split factory | 36.5 | 1.29 | 28× |
| bar median by line legend sensor | 127.9 | 2.05 | 62× |
| boxplot value by sensor_type | 213.9 | 4.56 | 47× |
| histogram value split sensor | 61.6 | 1.60 | 39× |
| heatmap ts×machine avg | 166.5 | 1.41 | 118× |
| legendValues sensor_type | 27.4 | 0.00 ※ | — |
| columnSummary machine | 24.1 | 0.09 | 267× |
| columnSummary ts | 51.8 | 0.30 | 174× |
| splitCount 3컬럼 | 1.3 ※※ | 2.01 | 0.6× |
| splitCount 필터 | 29.1 | 1.37 | 21× |
| sampleRows 필터 1페이지 | 99.1 | 0.25 | 395× |
| itemRows split 조합 1페이지 | 34.0 | 0.29 | 118× |

※ 네이티브는 범주 컬럼을 정렬된 사전(dictionary)으로 들고 있어 legend 값 목록이 이미 있다. 원본은 `select distinct`를 돈다. 구조 차이라 배율을 적지 않았다.
※※ 원본은 10만 행에서 이 케이스가 1.3ms로 이상하게 빠르다(100만 행은 347ms). 네이티브가 진 유일한 케이스지만 둘 다 사람이 느끼지 못하는 크기다.

## 3. draw 한 번 끝까지 (쿼리 + 렌더 + 섬네일 + PNG)

웹은 화면에서 draw를 누른 것과 같은 REST 요청을 보내고 job이 끝날 때까지(모든 item `success`) 잰 시간, 네이티브는 `vis-bench --png`로 같은 일을 잰 시간이다.

### 100만 행

| 시나리오 | item | 웹 쿼리 | 웹 전체 | 네이티브 쿼리 | 네이티브 전체 | 배율 |
|---|---:|---:|---:|---:|---:|---:|
| scatter ts×value | 1 | 1,794ms | 2,434ms | 2.7ms | 174ms | 14× |
| scatter split factory×line + legend | 12 | 2,468ms | 3,600ms | 18.8ms | 148ms | 24× |
| line split machine | 120 | 1,927ms | 2,919ms | 27.6ms | 371ms | 8× |
| bar avg machine split sensor×shift | 12 | 431ms | 526ms | 14.4ms | 47ms | 11× |
| boxplot sensor split machine | 120 | 4,039ms | 4,454ms | 23.8ms | 44ms | 102× |
| histogram value split lot_id | 4,941 | 1,319ms | 15,802ms | 23.2ms | 1,085ms | 15× |

### 10만 행

| 시나리오 | item | 웹 쿼리 | 웹 전체 | 네이티브 쿼리 | 네이티브 전체 | 배율 |
|---|---:|---:|---:|---:|---:|---:|
| scatter ts×value | 1 | 192ms | 432ms | 0.5ms | 24ms | 18× |
| scatter split factory×line + legend | 12 | 286ms | 563ms | 1.6ms | 26ms | 22× |
| line split machine | 120 | 236ms | 1,535ms | 1.7ms | 210ms | 7× |
| bar avg machine split sensor×shift | 12 | 105ms | 200ms | 2.1ms | 32ms | 6× |
| boxplot sensor split machine | 120 | 429ms | 801ms | 2.6ms | 21ms | 38× |
| histogram value split lot_id | 2,500 | 311ms | 7,221ms | 3.4ms | 511ms | 14× |

"웹 쿼리"는 job의 `timing.renderStartedAt − createdAt`(데이터 준비·검증·쿼리·결과 파싱)이다.

### 렌더만 떼어 보면 (100만 행, 전체 − 쿼리)

| 시나리오 | 웹 렌더 | 네이티브 렌더 | 배율 | item 하나에 쓴 시간 (렌더 스레드 3개) |
|---|---:|---:|---:|---|
| scatter 1 item (100만 점) | 640ms | 171ms | 3.7× | |
| scatter 12 items (약 8.3만 점씩) | 1,132ms | 129ms | 8.8× | |
| line 120 items (8,333점씩) | 992ms | 343ms | 2.9× | 약 25ms → 8.6ms |
| bar 12 items | 95ms | 33ms | 2.9× | |
| boxplot 120 items | 415ms | 20ms | 21× | |
| histogram 4,941 items | 14,483ms | 1,062ms | 13.6× | 약 8.8ms → 0.64ms |

렌더 쪽 이득은 쿼리보다 훨씬 작다. 브라우저의 OffscreenCanvas 2D는 Chromium의 Skia(C++)라 그 자체로 빠르고, 네이티브의 tiny-skia도 같은 종류의 CPU 래스터라이저다. 차이는 주로 item마다 붙는 고정 비용(Worker 메시지·transfer, `convertToBlob` PNG, Cache API 저장, MSW·JSON 응답, long polling)에서 난다. 그래서 item이 많고 그림이 단순할수록(boxplot·histogram) 배율이 크고, 그림이 무거울수록(line·scatter 1장) 작다.

참고로 네이티브 line 렌더러는 처음에 tiny-skia의 일반 stroke(외곽선으로 바꿔 채우기)를 써서 item당 17.6ms였다. 프로파일러에서 `fill_path_impl`이 절반을 넘어, 선 조각까지의 거리로 덮임을 계산하는 전용 래스터라이저로 바꿔 6.1ms가 됐다(`render/canvas.rs` `polyline_aa`). 네이티브로 옮겨도 렌더 쪽은 이런 손질이 필요하다는 뜻이다.

## 4. 앱 자체

| | 웹 | 네이티브 |
|---|---|---|
| 창이 뜰 때까지 | 부팅 화면: 화면 JS → MSW 등록 → PGlite 부팅(이 기기 Node 기준 3.5초) | 53ms (창 매핑까지, Linux/Xvfb) |
| 메모리 | 측정하지 않음 | 100만 행 등록 + 12 item draw 뒤 RSS 약 310MB (소프트웨어 Vulkan 드라이버 포함) |
| 배포 | 정적 호스팅 (서버 비용 0) | exe 하나 (release 약 60MB) |

## 해석할 때 주의할 점

- **같은 기기, Linux, GPU 없음**에서 쟀다. Windows 실기기 숫자는 아니다. GitHub Actions `build` 워크플로가 Windows 러너에서 `vis-bench --rows 100000`을 돌려 결과(`bench-windows-100k.json`)를 아티팩트로 남긴다. 사용자 PC에서는 앱의 **벤치마크** 화면으로 같은 케이스를 잴 수 있다(웹 기준값과 배율을 함께 보인다).
- 웹 쿼리 숫자는 Node에서 돈 PGlite다. 브라우저 DB Worker는 같은 엔진이지만 IndexedDB·Worker 메시지가 더해져 보통 더 느리다(데이터 등록이 Node 12.8초, 브라우저 24.0초였다). 그래서 쿼리 배율은 보수적인 쪽이다.
- 웹 draw는 MSW·JWT 검증·JSON·long polling 같은 "진짜 서버 흉내" 비용을 포함한다. 원본은 나중에 실서버로 옮길 수 있게 일부러 이 구조를 택했다. 네이티브는 이 경계를 없앴으므로 차이의 일부는 언어가 아니라 **아키텍처 선택**이다.
- 네이티브 렌더러는 원본의 기본 모양을 옮겼지만, 원본의 차트 속성 체계(축·글꼴·analytics·하위 차트 등)는 옮기지 않았다. 그리는 양은 비슷하게 맞췄지만(점은 픽셀 칸마다 3개까지, line은 M4) 완전히 같은 코드는 아니다.
- GPUI 화면 자체의 프레임 시간(스크롤·hover 반응성)은 이 컨테이너(소프트웨어 Vulkan)에서 의미 있게 잴 수 없어 숫자로 비교하지 않았다. 화면은 끝난 item을 33ms마다 가져가 비트맵을 그대로 GPU 텍스처로 올리므로 PNG 디코드가 없다.

## 재현

```bash
# 네이티브
cargo run --release -p vis-core --bin vis-bench -- --rows 1000000 --iterations 5 --out native.json
cargo run --release -p vis-core --bin vis-bench -- --rows 1000000 --skip-data --png --out native-png.json

# 웹 쿼리 (vis-a-lot 저장소)
pnpm bench:db --skip-feed --rows 1000000 --iterations 5 --out web.json

# 웹 draw (vis-a-lot 빌드를 띄운 뒤)
(cd ../vis-a-lot && pnpm build && npx serve out -l 4173)
node scripts/web-draw-bench.mjs http://localhost:4173 iot-1m web-draw.json
```
