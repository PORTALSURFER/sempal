import { env, createExecutionContext, waitOnExecutionContext, SELF } from 'cloudflare:test';
import { describe, it, expect } from 'vitest';
import worker from '../src/index';

// For now, you'll need to do something like this to get a correctly-typed
// `Request` to pass to `worker.fetch()`.
const IncomingRequest = Request<unknown, IncomingRequestCfProperties>;

describe('GitIssue Gateway Worker', () => {
	it('fails with 500 when config is missing', async () => {
		const request = new IncomingRequest('http://example.com/debug');
		const ctx = createExecutionContext();
		const response = await worker.fetch(request, env, ctx);
		await waitOnExecutionContext(ctx);
		expect(response.status).toBe(500);
		const data: any = await response.json();
		expect(data.error).toContain('Missing environment configuration');
	});

	it('responds with 404 for unknown routes', async () => {
		// Mock valid env for validation bypass
		const mockEnv = {
			...env,
			GITHUB_CLIENT_ID: 'id',
			GITHUB_CLIENT_SECRET: 'secret',
			GITHUB_OWNER: 'owner',
			GITHUB_REPO: 'repo',
			SESSIONS: (env as any).SESSIONS || { get: () => null }
		};
		const request = new IncomingRequest('http://example.com/unknown');
		const ctx = createExecutionContext();
		const response = await worker.fetch(request, mockEnv as any, ctx);
		await waitOnExecutionContext(ctx);
		expect(response.status).toBe(404);
	});
});
