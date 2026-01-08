// src/index.ts

const VERSION = "native-bearer-2";

// JSON response helper
function json(data: any, init: ResponseInit = {}): Response {
	return new Response(JSON.stringify(data, null, 2), {
		...init,
		headers: {
			"content-type": "application/json; charset=utf-8",
			...(init.headers ?? {})
		}
	});
}

// Base64 URL encoding
function base64url(bytes: Uint8Array): string {
	let bin = "";
	bytes.forEach((b) => (bin += String.fromCharCode(b)));
	return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
}

function randomToken(bytes = 32): string {
	const buf = new Uint8Array(bytes);
	crypto.getRandomValues(buf);
	return base64url(buf);
}

// Cookie parser
function getCookie(req: Request, name: string): string | null {
	const cookie = req.headers.get("cookie") || "";
	for (const part of cookie.split(";")) {
		const [k, ...rest] = part.trim().split("=");
		if (k === name) return decodeURIComponent(rest.join("="));
	}
	return null;
}

function getBearerToken(req: Request): string | null {
	const auth = req.headers.get("authorization") || "";
	if (auth.toLowerCase().startsWith("bearer ")) {
		const tok = auth.slice(7).trim();
		return tok.length > 0 ? tok : null;
	}
	return null;
}

function withCors(req: Request, res: Response): Response {
	const origin = req.headers.get("Origin") ?? "*";
	const headers = new Headers(res.headers);
	headers.set("Access-Control-Allow-Origin", origin);
	headers.set("Vary", "Origin");
	headers.set("Access-Control-Allow-Headers", "content-type, authorization");
	headers.set("Access-Control-Allow-Methods", "GET,POST,OPTIONS");
	return new Response(res.body, { ...res, headers });
}

interface Env {
	GITHUB_CLIENT_ID: string;
	GITHUB_CLIENT_SECRET: string;
	GITHUB_OWNER: string;
	GITHUB_REPO: string;
	SESSIONS: KVNamespace;
	APP_REDIRECT_URL?: string;
}

function validateEnv(env: Env) {
	const missing = [];
	if (!env.GITHUB_CLIENT_ID || env.GITHUB_CLIENT_ID.startsWith("YOUR_"))
		missing.push("GITHUB_CLIENT_ID");
	if (!env.GITHUB_CLIENT_SECRET) missing.push("GITHUB_CLIENT_SECRET");
	if (!env.GITHUB_OWNER || env.GITHUB_OWNER.startsWith("YOUR_"))
		missing.push("GITHUB_OWNER");
	if (!env.GITHUB_REPO || env.GITHUB_REPO.startsWith("YOUR_"))
		missing.push("GITHUB_REPO");
	if (!env.SESSIONS) missing.push("SESSIONS (KV binding)");

	if (missing.length > 0) {
		throw new Error(`Missing environment configuration: ${missing.join(", ")}`);
	}
}

async function exchangeCodeForToken(
	env: Env,
	code: string
): Promise<string> {
	const resp = await fetch("https://github.com/login/oauth/access_token", {
		method: "POST",
		headers: {
			"content-type": "application/json",
			"accept": "application/json",
			"user-agent": "sempal-gitissue-gateway"
		},
		body: JSON.stringify({
			client_id: env.GITHUB_CLIENT_ID,
			client_secret: env.GITHUB_CLIENT_SECRET,
			code
		})
	});
	const data: any = await resp.json();
	if (!resp.ok || data.error || !data.access_token) {
		throw new Error(
			`Token exchange failed: ${resp.status} ${JSON.stringify(data)}`
		);
	}
	return data.access_token as string;
}

async function createIssue(
	env: Env,
	userToken: string,
	title: string,
	body: string
) {
	const url = `https://api.github.com/repos/${encodeURIComponent(
		env.GITHUB_OWNER
	)}/${encodeURIComponent(env.GITHUB_REPO)}/issues`;
	const resp = await fetch(url, {
		method: "POST",
		headers: {
			"accept": "application/vnd.github+json",
			"content-type": "application/json",
			"user-agent": "sempal-gitissue-gateway",
			authorization: `Bearer ${userToken}`,
			"x-github-api-version": "2022-11-28"
		},
		body: JSON.stringify({ title, body })
	});
	const data: any = await resp.json();
	if (!resp.ok) {
		throw new Error(
			`GitHub create issue failed: ${resp.status} ${JSON.stringify(data)}`
		);
	}
	return data;
}

export default {
	async fetch(req: Request, env: Env, ctx: any): Promise<Response> {
		try {
			validateEnv(env);
		} catch (e) {
			return json({ error: String(e) }, { status: 500 });
		}

		const url = new URL(req.url);

		if (req.method === "OPTIONS") {
			return withCors(req, new Response(null, { status: 204 }));
		}

		if (req.method === "GET" && url.pathname === "/debug") {
			return withCors(
				req,
				json({
					ok: true,
					version: VERSION,
					target: `${env.GITHUB_OWNER}/${env.GITHUB_REPO}`,
					hasAuthHeader: Boolean(req.headers.get("authorization")),
					configured: true
				})
			);
		}

		if (req.method === "GET" && url.pathname === "/auth/start") {
			const state = randomToken(24);
			await env.SESSIONS.put(`state:${state}`, "1", { expirationTtl: 600 });
			const authUrl = new URL("https://github.com/login/oauth/authorize");
			authUrl.searchParams.set("client_id", env.GITHUB_CLIENT_ID);
			authUrl.searchParams.set("state", state);
			return Response.redirect(authUrl.toString(), 302);
		}

		if (req.method === "GET" && url.pathname === "/auth/callback") {
			const code = url.searchParams.get("code");
			const state = url.searchParams.get("state");
			if (!code || !state) {
				return json({ error: "Missing code/state" }, { status: 400 });
			}
			const stateKey = `state:${state}`;
			const ok = await env.SESSIONS.get(stateKey);
			if (!ok) {
				return json({ error: "Invalid/expired state" }, { status: 400 });
			}
			await env.SESSIONS.delete(stateKey);
			let githubToken: string;
			try {
				githubToken = await exchangeCodeForToken(env, code);
			} catch (e) {
				return json({ error: String(e) }, { status: 500 });
			}
			const sessionId = randomToken(32);
			await env.SESSIONS.put(`sess:${sessionId}`, githubToken, {
				expirationTtl: 60 * 60 * 24 * 7
			});
			const cookie = `sempal_sess=${encodeURIComponent(
				sessionId
			)}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=${60 * 60 * 24 * 7
				}`;

			if (env.APP_REDIRECT_URL?.trim()) {
				return new Response(null, {
					status: 302,
					headers: { "set-cookie": cookie, location: env.APP_REDIRECT_URL }
				});
			}

			return new Response(
				`✔ GitHub connected

Copy this token into the app:

${sessionId}

You can close this tab.`,
				{
					status: 200,
					headers: {
						"content-type": "text/plain; charset=utf-8",
						"set-cookie": cookie
					}
				}
			);
		}

		if (req.method === "POST" && url.pathname === "/issue") {
			const sessionId =
				getBearerToken(req) || getCookie(req, "sempal_sess");
			if (!sessionId) {
				return withCors(
					req,
					json({ error: "Not authenticated" }, { status: 401 })
				);
			}
			const githubToken = await env.SESSIONS.get(`sess:${sessionId}`);
			if (!githubToken) {
				return withCors(
					req,
					json({ error: "Session expired" }, { status: 401 })
				);
			}

			let payload: any;
			try {
				payload = await req.json();
			} catch {
				return withCors(
					req,
					json({ error: "Invalid JSON" }, { status: 400 })
				);
			}

			const title = String(payload?.title ?? "").trim();
			const body = payload?.body != null ? String(payload.body) : "";
			if (title.length < 3 || title.length > 200) {
				return withCors(
					req,
					json(
						{ error: "Title must be 3–200 chars" },
						{ status: 400 }
					)
				);
			}

			try {
				const issue = await createIssue(
					env,
					githubToken,
					title,
					body
				);
				return withCors(
					req,
					json(
						{ ok: true, issue_url: issue.html_url, number: issue.number },
						{ status: 200 }
					)
				);
			} catch (e) {
				return withCors(
					req,
					json({ error: String(e) }, { status: 500 })
				);
			}
		}

		return new Response("Not found", { status: 404 });
	}
};
