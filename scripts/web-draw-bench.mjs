// vis-a-lot(웹, CSR) draw 벤치마크: 프로덕션 빌드를 Chromium에서 띄우고, 화면과 같은 REST 요청(MSW → Hono 서비스
// → PGlite DB Worker → Render Worker × N → PNG)으로 draw를 끝까지 잰다. vis-bench의 draw 시나리오와 같은 설정이다.
//
// 사용: node scripts/web-draw-bench.mjs <vis-a-lot out/ 를 서빙하는 URL> [iot-100k|iot-1m] [결과.json]
// 예:   (cd ../vis-a-lot && pnpm build && npx serve out -l 4173) 후 node scripts/web-draw-bench.mjs http://localhost:4173 iot-1m

import { writeFileSync } from "node:fs";
import { createRequire } from "node:module";

// playwright는 전역 설치를 쓰는 환경이 많아 require로 찾는다 (NODE_PATH도 따른다)
const { chromium } = createRequire(import.meta.url)("playwright");

const base = process.argv[2] ?? "http://localhost:4173";
const dataset = process.argv[3] ?? "iot-100k";
const out = process.argv[4];

const SCENARIOS = [
  ["scatter ts×value (1 item)", { chartType: "scatter", x: "ts", y: "value", splitBy: [], legend: null }],
  ["scatter split factory×line + legend (12 items)", { chartType: "scatter", x: "ts", y: "value", splitBy: ["factory", "line"], legend: "sensor_type" }],
  ["line split machine (120 items)", { chartType: "line", x: "ts", y: "value", splitBy: ["machine"], legend: null }],
  ["bar avg machine split sensor×shift (12 items)", { chartType: "bar", x: "machine", y: "value", splitBy: ["sensor_type", "shift"], legend: null }],
  ["boxplot sensor split machine (120 items)", { chartType: "boxplot", x: "sensor_type", y: "value", splitBy: ["machine"], legend: null }],
  ["histogram value split lot_id (~5,000 items)", { chartType: "histogram", x: "value", y: "value", splitBy: ["lot_id"], legend: null }],
];

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
page.on("console", (m) => {
  if (m.type() === "error") console.error("[page]", m.text());
});
await page.goto(base, { waitUntil: "load" });
// MSW service worker가 페이지를 잡을 때까지 (부팅 화면이 끝날 때까지)
await page.waitForFunction(() => navigator.serviceWorker?.controller != null, null, { timeout: 120_000 });
console.log("service worker ready");

const result = await page.evaluate(
  async ({ dataset, scenarios }) => {
    const j = async (path, init = {}, token) => {
      const res = await fetch(path, {
        ...init,
        headers: { "content-type": "application/json", ...(token ? { authorization: `Bearer ${token}` } : {}), ...(init.headers ?? {}) },
      });
      const text = await res.text();
      if (!res.ok) throw new Error(`${init.method ?? "GET"} ${path} ${res.status}: ${text.slice(0, 300)}`);
      return text ? JSON.parse(text) : null;
    };
    const login = await j("/api/auth/login", { method: "POST", body: JSON.stringify({ username: "demo", password: "demo1234" }) });
    let token = login.accessToken;
    let refresh = login.refreshToken;
    const renew = async () => {
      const r = await j("/api/auth/refresh", { method: "POST", body: JSON.stringify({ refreshToken: refresh }) });
      token = r.accessToken;
      refresh = r.refreshToken;
    };
    const call = async (path, init) => {
      try {
        return await j(path, init, token);
      } catch (e) {
        if (String(e).includes(" 401")) {
          await renew();
          return j(path, init, token);
        }
        throw e;
      }
    };
    // 데이터셋 등록 (샘플 생성 + COPY 적재)
    const t0 = performance.now();
    await call(`/api/data/datasets/${dataset}/generate`, { method: "POST", body: "{}" }).catch((e) => console.log("generate:", String(e)));
    for (;;) {
      const list = await call("/api/data/datasets");
      const items = Array.isArray(list) ? list : (list.datasets ?? list.items ?? []);
      const d = items.find((x) => x.id === dataset);
      const st = d?.state ?? d?.status;
      if (st === "ready") break;
      if (st === "failed") throw new Error(`dataset failed: ${JSON.stringify(d)}`);
      await new Promise((r) => setTimeout(r, 250));
    }
    const loadMs = performance.now() - t0;
    const draws = [];
    for (const [name, s] of scenarios) {
      const runs = [];
      for (let k = 0; k < 2; k++) {
        const started = performance.now();
        const spec = {
          dataset,
          chartType: s.chartType,
          x: s.x,
          y: s.y,
          agg: "avg",
          legend: s.legend,
          splitBy: s.splitBy,
          splitSeparator: "-",
          tooltip: [],
          grid: { rows: 3, cols: 3 },
          common: { color: "#2a78d6" },
          filters: [],
        };
        const created = await call("/api/chart/charts", {
          method: "POST",
          body: JSON.stringify({ spec, itemSize: { width: 400, height: 300, pixelRatio: 1, theme: "light" }, record: false }),
        });
        let st;
        for (;;) {
          st = await call(`/api/chart/jobs/${created.jobId}?page=0&pageSize=1`);
          if (["success", "failed", "cancelled"].includes(st.status)) break;
          await new Promise((r) => setTimeout(r, 30));
        }
        const total = performance.now() - started;
        const t = st.timing ?? {};
        runs.push({
          name,
          items: st.totalItems ?? st.progress.total,
          status: st.status,
          workers: st.renderWorkers,
          totalMs: total,
          queryMs: t.renderStartedAt && t.createdAt ? t.renderStartedAt - t.createdAt : null,
          serverMs: t.endedAt && t.createdAt ? t.endedAt - t.createdAt : null,
          counts: st.counts,
        });
      }
      // 첫 회는 warmup (Worker·캐시), 두 번째를 쓴다
      draws.push(runs[1]);
    }
    return { loadMs, draws, cores: navigator.hardwareConcurrency, ua: navigator.userAgent };
  },
  { dataset, scenarios: SCENARIOS },
);

for (const d of result.draws) {
  console.log(
    `draw ${d.name.padEnd(48)} ${String(d.items).padStart(6)} items  query ${(d.queryMs ?? 0).toFixed(1).padStart(8)} ms  total ${d.totalMs.toFixed(1).padStart(9)} ms  ${(d.items / (d.totalMs / 1000)).toFixed(0).padStart(6)} item/s  (${d.status}, workers ${d.workers})`,
  );
}
console.log(`dataset ${dataset} registered in ${result.loadMs.toFixed(0)} ms · cores ${result.cores}`);
if (out) writeFileSync(out, JSON.stringify({ dataset, at: new Date().toISOString(), ...result }, null, 2));
await browser.close();
