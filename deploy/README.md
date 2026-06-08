# WatchDog Deployment

WatchDog has two deployment tracks:

- `vercel-demo/`: static GTM preview for Vercel.
- Root Docker image: live Rust dashboard service for Render, Railway, Fly.io, or any Docker host.

Use the Vercel preview as the easy public product overview. Use the Docker service when you want the hosted app to behave end to end with backend-generated incidents, saved notes, status updates, cached explanations, exports, health checks, and Supabase or SQLite persistence.

## Vercel static preview

Deploy the static demo page:

```bash
npx vercel deploy vercel-demo -y
```

This is the best lightweight public demo link. It does not run the Rust daemon; it presents the product story, incident narrative, evidence, Slack alert, and workflow in a Vercel-friendly static page.

## Docker dashboard service

Build and run the live Rust dashboard with seeded demo data:

```bash
docker build -t watchdog-demo .
docker run --rm -p 3000:3000 watchdog-demo
```

Open `http://localhost:3000`.

From the dashboard, use the scenario buttons to create new persisted incidents. Each scenario sends synthetic deploy, metric, and log events through the Rust detector, then saves the resulting incident in the configured storage backend.

Health check:

```bash
curl http://localhost:3000/healthz
```

The container uses the built-in lightweight incident explainer by default:

```bash
WATCHDOG_EXPLAINER=local
```

For Ollama-backed explanations, run an Ollama service that the container can reach and set:

```bash
WATCHDOG_EXPLAINER=ollama
WATCHDOG_OLLAMA_BASE_URL=http://host.docker.internal:11434/api
WATCHDOG_OLLAMA_MODEL=gemma3
```

## Render

Use `deploy/render.yaml` as a Blueprint. It builds from the root `Dockerfile`, seeds a demo incident on start, and serves the dashboard on Render's provided `PORT`.

Recommended environment:

```bash
WATCHDOG_EXPLAINER=local
WATCHDOG_STORAGE=supabase
SUPABASE_URL=https://your-project.supabase.co
SUPABASE_SERVICE_ROLE_KEY=your-service-role-key
```

With Supabase storage, Render does not need a persistent disk for incident data. Attach a disk only if you also want local JSONL demo inputs or SQLite fallback files to survive restarts.
Use the root [`.env.example`](../.env.example) as the starting point for hosted environment variables.

## Railway

Use `deploy/railway.json` with the root `Dockerfile`. Railway provides `PORT`, so the Docker command will serve the dashboard correctly.

Recommended variables:

```bash
WATCHDOG_EXPLAINER=local
WATCHDOG_STORAGE=supabase
SUPABASE_URL=https://your-project.supabase.co
SUPABASE_SERVICE_ROLE_KEY=your-service-role-key
```

With Supabase storage, Railway does not need a volume for incident history.

## Demo database

The cloud demo can use Supabase Postgres through the Supabase REST API:

```bash
WATCHDOG_STORAGE=supabase
SUPABASE_URL=https://your-project.supabase.co
SUPABASE_SERVICE_ROLE_KEY=your-service-role-key
```

Create this table in Supabase SQL Editor:

```sql
create table if not exists incidents (
  id text primary key,
  created_at timestamptz not null,
  severity text not null,
  status text not null default 'open',
  deploy_id text not null,
  environment text not null,
  summary text not null,
  incident_json jsonb not null,
  updated_at timestamptz not null default now()
);

create index if not exists idx_incidents_created_at
  on incidents (created_at desc);

create index if not exists idx_incidents_status
  on incidents (status);

create index if not exists idx_incidents_deploy_id
  on incidents (deploy_id);
```

The application still supports `WATCHDOG_STORAGE=sqlite` and `WATCHDOG_STORAGE=json-files` for local demos.

## Production note

For a real production version, split the architecture:

- Frontend on Vercel.
- Long-running ingestion service on a Docker host.
- Durable state in Postgres, Supabase, Neon, Redis, or object storage.
- Webhook or queue-based metric/deploy ingestion instead of local JSONL tailing.
