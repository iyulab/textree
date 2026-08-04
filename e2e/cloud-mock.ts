import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";

/**
 * Stand-in for the publishing backend, so the in-app sign-in can be driven end to end without
 * reaching the real service.
 *
 * The token exchange rejects by default and grants only when a test asks it to (`grantToken`), so
 * a test that means to cover the failure path cannot accidentally complete a sign-in. Completing
 * one is safe because a development build stores credentials under its own namespace and cannot
 * reach the installed app's entry (see `secret_store.rs`); `credential-probe.ts` checks that
 * separation still holds rather than trusting it.
 *
 * Ports are fixed because the app reads its base URLs from the environment at launch, before a
 * test process exists to negotiate them.
 */

export const MOCK_APP_PORT = 8787;
export const MOCK_API_PORT = 8788;
export const MOCK_APP_BASE = `http://127.0.0.1:${MOCK_APP_PORT}`;
export const MOCK_API_BASE = `http://127.0.0.1:${MOCK_API_PORT}`;

/** The query the desktop put on the authorize URL. */
export interface AuthorizeParams {
  /** Loopback port the desktop is listening on. */
  port: string;
  state: string;
  codeChallenge: string;
  codeChallengeMethod: string;
}

export interface CloudMock {
  /** Waits for the desktop to open the authorize URL and returns its query. */
  waitForAuthorize(timeoutMs?: number): Promise<AuthorizeParams>;
  /** Bodies the desktop POSTed to the token endpoint, oldest first. */
  exchangeBodies(): unknown[];
  /** Makes the next exchanges succeed with this token. Rejection is the default. */
  grantToken(token: string): void;
  /** Forgets what was recorded and reverts to rejecting, so the next sign-in starts clean. */
  reset(): void;
  close(): Promise<void>;
}

function listen(server: Server, port: number): Promise<void> {
  return new Promise((res, rej) => {
    server.once("error", rej);
    server.listen(port, "127.0.0.1", () => res());
  });
}

function close(server: Server): Promise<void> {
  // The browser that opened the stub page holds its socket open, and close() alone waits for every
  // live connection — which would hang teardown until the tab goes away.
  server.closeAllConnections();
  return new Promise((res) => server.close(() => res()));
}

function readBody(req: IncomingMessage): Promise<string> {
  return new Promise((res) => {
    let raw = "";
    req.on("data", (c) => (raw += c));
    req.on("end", () => res(raw));
  });
}

export async function startCloudMock(): Promise<CloudMock> {
  let authorize: AuthorizeParams | null = null;
  let granted: string | null = null;
  const exchanges: unknown[] = [];

  const app = createServer((req: IncomingMessage, res: ServerResponse) => {
    const url = new URL(req.url ?? "/", MOCK_APP_BASE);
    if (url.pathname === "/connect/desktop") {
      authorize = {
        port: url.searchParams.get("port") ?? "",
        state: url.searchParams.get("state") ?? "",
        codeChallenge: url.searchParams.get("code_challenge") ?? "",
        codeChallengeMethod: url.searchParams.get("code_challenge_method") ?? "",
      };
      // Deliberately no redirect: the test delivers the callback itself, so the assertions do not
      // depend on how the machine's default browser handles a redirect to a loopback address.
      res.writeHead(200, { "content-type": "text/html; charset=utf-8", connection: "close" });
      res.end("<!doctype html><title>sign-in stub</title><p>Test stub — this tab can be closed.</p>");
      return;
    }
    res.writeHead(404).end();
  });

  const api = createServer(async (req: IncomingMessage, res: ServerResponse) => {
    const url = new URL(req.url ?? "/", MOCK_API_BASE);
    if (req.method === "POST" && url.pathname === "/oauth/desktop/token") {
      const raw = await readBody(req);
      try {
        exchanges.push(JSON.parse(raw));
      } catch {
        exchanges.push(raw);
      }
      if (granted !== null) {
        res.writeHead(200, { "content-type": "application/json", connection: "close" });
        res.end(JSON.stringify({ token: granted }));
        return;
      }
      res.writeHead(400, { "content-type": "application/json", connection: "close" });
      res.end(JSON.stringify({ error: "invalid_grant" }));
      return;
    }
    res.writeHead(404).end();
  });

  await listen(app, MOCK_APP_PORT);
  await listen(api, MOCK_API_PORT);

  return {
    async waitForAuthorize(timeoutMs = 15_000): Promise<AuthorizeParams> {
      const deadline = Date.now() + timeoutMs;
      while (Date.now() < deadline) {
        if (authorize) return authorize;
        await new Promise((r) => setTimeout(r, 100));
      }
      throw new Error(
        `The desktop never opened the authorize URL on ${MOCK_APP_BASE}. ` +
          `Launch the app with TEXTREE_APP_BASE=${MOCK_APP_BASE} and TEXTREE_API_BASE=${MOCK_API_BASE}.`,
      );
    },
    exchangeBodies: () => exchanges.slice(),
    grantToken(token: string) {
      granted = token;
    },
    reset() {
      authorize = null;
      granted = null;
      exchanges.length = 0;
    },
    async close() {
      await Promise.all([close(app), close(api)]);
    },
  };
}

/** Delivers a loopback callback the way the browser would, and returns the HTTP status. */
export async function deliverCallback(port: string, code: string, state: string): Promise<number> {
  const res = await fetch(`http://127.0.0.1:${port}/callback?code=${encodeURIComponent(code)}&state=${encodeURIComponent(state)}`);
  return res.status;
}
