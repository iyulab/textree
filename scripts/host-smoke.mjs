#!/usr/bin/env node
/**
 * Host smoke: start the real AI helper and make it do its whole job once.
 *
 * The host's unit tests replace the text generator with a stub, and its /health reports the
 * generator as not ready until something asks for it — so a helper that cannot load its local
 * model at all still passes every test and reports healthy. This starts the actual process, asks
 * it to prepare generation, indexes two notes, searches them, and generates a reply. Any step
 * failing fails the run.
 *
 * Needs the local models already cached (a first run downloads several GB and is not what this
 * checks), so it is a local and pre-release check rather than a CI one.
 *
 *   npm run host:smoke                                         # the project, via dotnet run
 *   npm run host:smoke -- --exe src-tauri/resources/host/textree-host.exe   # the shipped build
 *
 * The second form checks what an installer carries: a single self-extracting file whose native
 * libraries only appear at launch, which `dotnet run` never exercises.
 */
import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const PROJECT = join(REPO, "src-host", "src", "Textree.Host");
const READY_TIMEOUT_MS = 5 * 60_000;
const exeArg = process.argv.indexOf("--exe");
const EXE = exeArg > 0 ? resolve(process.argv[exeArg + 1]) : null;

const freePort = () =>
  new Promise((ok, fail) => {
    const s = createServer();
    s.once("error", fail);
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => ok(port));
    });
  });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function fail(message) {
  console.error(`[host:smoke] FAILED — ${message}`);
  process.exitCode = 1;
  throw new Error(message);
}

const port = await freePort();
const base = `http://127.0.0.1:${port}`;
const vault = mkdtempSync(join(tmpdir(), "textree-host-smoke-"));
writeFileSync(join(vault, "garden.md"), "# Garden\nTomatoes need full sun and regular watering.\n");
writeFileSync(join(vault, "taxes.md"), "# Taxes\nFile the quarterly estimate before the deadline.\n");
const slashed = vault.replace(/\\/g, "/");

console.log(`[host:smoke] starting ${EXE ?? "the host project"} on ${base}`);
const host = EXE
  ? spawn(EXE, ["--urls", base], { stdio: ["ignore", "pipe", "pipe"] })
  : spawn("dotnet", ["run", "--project", PROJECT, "--", "--urls", base], {
      stdio: ["ignore", "pipe", "pipe"],
      shell: process.platform === "win32",
    });
let output = "";
host.stdout.on("data", (d) => (output += d));
host.stderr.on("data", (d) => (output += d));

const post = (path, body) =>
  fetch(base + path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });

async function health() {
  try {
    return await (await fetch(`${base}/health`)).json();
  } catch {
    return null;
  }
}

try {
  const deadline = Date.now() + READY_TIMEOUT_MS;
  while (!(await health())) {
    if (Date.now() > deadline) fail("the host never answered /health");
    if (host.exitCode !== null) fail(`the host exited (${host.exitCode}):\n${output.slice(-2000)}`);
    await sleep(1000);
  }

  const prepared = await post("/prepare-generation");
  if (!prepared.ok) fail(`/prepare-generation answered ${prepared.status}`);

  let h;
  for (;;) {
    h = await health();
    if (h?.generatorError) fail(`the generator cannot load: ${h.generatorError}`);
    if (h?.embedderError) fail(`the embedder cannot load: ${h.embedderError}`);
    if (h?.generatorReady && h?.embedderReady) break;
    if (Date.now() > deadline) fail(`not ready in time: ${JSON.stringify(h)}`);
    await sleep(2000);
  }
  console.log("[host:smoke] embedder and generator ready");
  console.log(`[host:smoke] runs on — embedder: ${JSON.stringify(h.embedderProviders)}, generator: ${JSON.stringify(h.generatorProviders)}`);

  for (const name of ["garden.md", "taxes.md"]) {
    const r = await post("/index", { vaultPath: slashed, path: `${slashed}/${name}` });
    if (!r.ok) fail(`/index ${name} answered ${r.status}`);
  }
  const search = await (
    await post("/search", { vaultPath: slashed, query: "growing vegetables in sunlight", scopePath: slashed, limit: 2 })
  ).json();
  if (search.results?.[0]?.path !== "garden.md") fail(`search ranked wrong: ${JSON.stringify(search)}`);
  console.log("[host:smoke] search finds the right note first");

  const chat = await post("/chat", {
    messages: [{ role: "user", content: "Reply with exactly one word: pong" }],
    maxTokens: 16,
  });
  const text = await chat.text();
  if (!chat.ok) fail(`/chat answered ${chat.status}: ${text.slice(0, 300)}`);
  if (!/"content":"[^"]+"/.test(text)) fail(`/chat returned no content: ${text.slice(0, 300)}`);
  console.log("[host:smoke] local generation answers");
  console.log("[host:smoke] OK");
} finally {
  await post("/shutdown").catch(() => {});
  for (let i = 0; i < 20 && host.exitCode === null; i++) await sleep(500);
  // `dotnet run` goes through a shell on Windows: killing the shell leaves the host running,
  // holding the build output locked. End the whole tree.
  if (host.exitCode === null) {
    if (process.platform === "win32") spawnSync("taskkill", ["/PID", String(host.pid), "/T", "/F"]);
    else host.kill();
  }
  rmSync(vault, { recursive: true, force: true });
}
