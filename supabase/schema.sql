-- todo-helper's shared calendar. Run all of it in the SQL editor of the Supabase project.
-- Running it again is safe: it only adds what is missing and replaces the rules and triggers.
--
-- Members sign in with email and password (public sign-up is off, see SETUP.md). A signed-in
-- member reads every event and profile, adds events, and changes or deletes any event. Every
-- add, change, and delete lands in event_log, which members can read but never write.
--
-- Everything lives in its own schema, todo_helper, so it stays apart from other tables in the
-- same project. The API only serves it once todo_helper is in Exposed schemas (SETUP.md).

create schema if not exists todo_helper;
revoke all on schema todo_helper from public, anon;
grant usage on schema todo_helper to authenticated;

-- Each member's name, as the others see it.
create table if not exists todo_helper.profiles (
  id uuid primary key references auth.users (id) on delete cascade,
  name text not null check (char_length(btrim(name)) between 1 and 40)
);

create table if not exists todo_helper.events (
  id uuid primary key default gen_random_uuid(),
  title text not null check (char_length(btrim(title)) between 1 and 200),
  starts_at timestamptz not null,
  -- For an all-day event, the start of its last day.
  ends_at timestamptz not null,
  all_day boolean not null default false,
  note text not null default '' check (char_length(note) <= 4000),
  -- Set by stamp_event from the signed-in member, never by the app.
  created_by uuid references auth.users (id) on delete set null,
  created_at timestamptz not null default now(),
  updated_by uuid references auth.users (id) on delete set null,
  updated_at timestamptz not null default now(),
  check (ends_at >= starts_at)
);
create index if not exists events_by_time on todo_helper.events (starts_at, ends_at);

-- Every add, change, and delete, written by log_event. A delete keeps the whole event in `before`,
-- so it can be put back from the dashboard.
create table if not exists todo_helper.event_log (
  id bigint generated always as identity primary key,
  event_id uuid not null,
  action text not null check (action in ('added', 'changed', 'deleted')),
  by_user uuid references auth.users (id) on delete set null,
  at timestamptz not null default now(),
  before jsonb,
  after jsonb
);

-- The database says who added and who last changed an event. Outside a member's sign-in (the
-- dashboard), the values given are kept, so the owner can put back a deleted event as it was.
create or replace function todo_helper.stamp_event() returns trigger
language plpgsql set search_path = '' as $$
begin
  if tg_op = 'INSERT' then
    new.created_by := coalesce(auth.uid(), new.created_by);
    new.created_at := now();
  else
    new.id := old.id;
    new.created_by := old.created_by;
    new.created_at := old.created_at;
  end if;
  new.updated_by := coalesce(auth.uid(), new.updated_by);
  new.updated_at := now();
  return new;
end $$;

drop trigger if exists stamp on todo_helper.events;
create trigger stamp before insert or update on todo_helper.events
  for each row execute function todo_helper.stamp_event();

-- Runs as the table owner, because members may not write to the log themselves.
create or replace function todo_helper.log_event() returns trigger
language plpgsql security definer set search_path = '' as $$
begin
  if tg_op = 'INSERT' then
    insert into todo_helper.event_log (event_id, action, by_user, after) values (new.id, 'added', new.created_by, to_jsonb(new));
  elsif tg_op = 'UPDATE' then
    insert into todo_helper.event_log (event_id, action, by_user, before, after) values (new.id, 'changed', new.updated_by, to_jsonb(old), to_jsonb(new));
  else
    insert into todo_helper.event_log (event_id, action, by_user, before) values (old.id, 'deleted', auth.uid(), to_jsonb(old));
  end if;
  return null;
end $$;

drop trigger if exists log on todo_helper.events;
create trigger log after insert or update or delete on todo_helper.events
  for each row execute function todo_helper.log_event();

-- Rules. Someone who isn't signed in gets nothing; the log is read-only for members.
alter table todo_helper.profiles enable row level security;
alter table todo_helper.events enable row level security;
alter table todo_helper.event_log enable row level security;

revoke all on todo_helper.profiles, todo_helper.events, todo_helper.event_log from anon;
revoke all on todo_helper.profiles, todo_helper.events, todo_helper.event_log from authenticated;
grant select, insert, update on todo_helper.profiles to authenticated;
grant select, insert, update, delete on todo_helper.events to authenticated;
grant select on todo_helper.event_log to authenticated;

drop policy if exists "members read profiles" on todo_helper.profiles;
create policy "members read profiles" on todo_helper.profiles for select to authenticated using (true);
drop policy if exists "members add their own name" on todo_helper.profiles;
create policy "members add their own name" on todo_helper.profiles for insert to authenticated
  with check (id = (select auth.uid()));
drop policy if exists "members change their own name" on todo_helper.profiles;
create policy "members change their own name" on todo_helper.profiles for update to authenticated
  using (id = (select auth.uid())) with check (id = (select auth.uid()));

drop policy if exists "members read events" on todo_helper.events;
create policy "members read events" on todo_helper.events for select to authenticated using (true);
drop policy if exists "members add events" on todo_helper.events;
create policy "members add events" on todo_helper.events for insert to authenticated with check (true);
drop policy if exists "members change events" on todo_helper.events;
create policy "members change events" on todo_helper.events for update to authenticated using (true) with check (true);
drop policy if exists "members delete events" on todo_helper.events;
create policy "members delete events" on todo_helper.events for delete to authenticated using (true);

drop policy if exists "members read the log" on todo_helper.event_log;
create policy "members read the log" on todo_helper.event_log for select to authenticated using (true);

-- ---- Tasks members give each other (added 2026-10-27) ----
-- Same rules as events: every member reads, gives, changes, and deletes any task, and every
-- change lands in task_log. created_by is who gave the task; stamp_event sets it.
create table if not exists todo_helper.tasks (
  id uuid primary key default gen_random_uuid(),
  title text not null check (char_length(btrim(title)) between 1 and 200),
  note text not null default '' check (char_length(note) <= 4000),
  assignee uuid not null references auth.users (id) on delete cascade,
  due date not null,
  done boolean not null default false,
  created_by uuid references auth.users (id) on delete set null,
  created_at timestamptz not null default now(),
  updated_by uuid references auth.users (id) on delete set null,
  updated_at timestamptz not null default now()
);

create table if not exists todo_helper.task_log (
  id bigint generated always as identity primary key,
  task_id uuid not null,
  action text not null check (action in ('added', 'changed', 'deleted')),
  by_user uuid references auth.users (id) on delete set null,
  at timestamptz not null default now(),
  before jsonb,
  after jsonb
);

drop trigger if exists stamp on todo_helper.tasks;
create trigger stamp before insert or update on todo_helper.tasks
  for each row execute function todo_helper.stamp_event();

create or replace function todo_helper.log_task() returns trigger
language plpgsql security definer set search_path = '' as $$
begin
  if tg_op = 'INSERT' then
    insert into todo_helper.task_log (task_id, action, by_user, after) values (new.id, 'added', new.created_by, to_jsonb(new));
  elsif tg_op = 'UPDATE' then
    insert into todo_helper.task_log (task_id, action, by_user, before, after) values (new.id, 'changed', new.updated_by, to_jsonb(old), to_jsonb(new));
  else
    insert into todo_helper.task_log (task_id, action, by_user, before) values (old.id, 'deleted', auth.uid(), to_jsonb(old));
  end if;
  return null;
end $$;

drop trigger if exists log on todo_helper.tasks;
create trigger log after insert or update or delete on todo_helper.tasks
  for each row execute function todo_helper.log_task();

alter table todo_helper.tasks enable row level security;
alter table todo_helper.task_log enable row level security;
revoke all on todo_helper.tasks, todo_helper.task_log from anon, authenticated;
grant select, insert, update, delete on todo_helper.tasks to authenticated;
grant select on todo_helper.task_log to authenticated;

drop policy if exists "members read tasks" on todo_helper.tasks;
create policy "members read tasks" on todo_helper.tasks for select to authenticated using (true);
drop policy if exists "members give tasks" on todo_helper.tasks;
create policy "members give tasks" on todo_helper.tasks for insert to authenticated with check (true);
drop policy if exists "members change tasks" on todo_helper.tasks;
create policy "members change tasks" on todo_helper.tasks for update to authenticated using (true) with check (true);
drop policy if exists "members delete tasks" on todo_helper.tasks;
create policy "members delete tasks" on todo_helper.tasks for delete to authenticated using (true);
drop policy if exists "members read the task log" on todo_helper.task_log;
create policy "members read the task log" on todo_helper.task_log for select to authenticated using (true);
