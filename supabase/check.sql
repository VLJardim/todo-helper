-- Checks the shared calendar's rules. Run all of it in the SQL editor, after schema.sql.
-- It acts as two made-up members (A and B) and as someone who isn't signed in, stops with
-- "FAILED: ..." if a rule lets too much or too little through, and removes everything it made.
-- On success the result is one row: "All checks passed".
do $$
declare
  a uuid := gen_random_uuid();
  b uuid := gen_random_uuid();
  ev todo_helper.events;
  n int;
  actions text;
  tk uuid;
  by_a uuid;
  by_b uuid;
begin
  insert into auth.users (id, email, aud, role) values
    (a, a || '@check.invalid', 'authenticated', 'authenticated'),
    (b, b || '@check.invalid', 'authenticated', 'authenticated');
  insert into todo_helper.profiles (id, name) values (a, 'Check A'), (b, 'Check B');

  -- Someone who isn't signed in reads and adds nothing.
  perform set_config('request.jwt.claims', '{"role": "anon"}', true);
  set local role anon;
  begin
    perform 1 from todo_helper.events;
    raise exception 'FAILED: someone who isn''t signed in can read events';
  exception when insufficient_privilege then null;
  end;
  begin
    perform 1 from todo_helper.event_log;
    raise exception 'FAILED: someone who isn''t signed in can read the log';
  exception when insufficient_privilege then null;
  end;
  begin
    insert into todo_helper.events (title, starts_at, ends_at) values ('x', now(), now());
    raise exception 'FAILED: someone who isn''t signed in can add an event';
  exception when insufficient_privilege then null;
  end;

  -- A adds an event and can't pretend B did.
  reset role;
  perform set_config('request.jwt.claims', json_build_object('sub', a, 'role', 'authenticated')::text, true);
  set local role authenticated;
  insert into todo_helper.events (title, starts_at, ends_at, created_by, updated_by)
    values ('Check meeting', now(), now() + interval '1 hour', b, b) returning * into ev;
  if ev.created_by is distinct from a or ev.updated_by is distinct from a then
    raise exception 'FAILED: the app could set who added an event';
  end if;
  select count(*) into n from todo_helper.profiles where id in (a, b);
  if n <> 2 then raise exception 'FAILED: a member can''t read every profile'; end if;

  -- B reads, changes, and deletes A's event, but can't touch A's name or the log.
  reset role;
  perform set_config('request.jwt.claims', json_build_object('sub', b, 'role', 'authenticated')::text, true);
  set local role authenticated;
  select count(*) into n from todo_helper.events where id = ev.id;
  if n <> 1 then raise exception 'FAILED: a member can''t read another member''s event'; end if;
  update todo_helper.events set title = 'Check meeting, moved', created_by = b where id = ev.id returning * into ev;
  if ev.title <> 'Check meeting, moved' then raise exception 'FAILED: a member can''t change another member''s event'; end if;
  if ev.created_by is distinct from a or ev.updated_by is distinct from b then
    raise exception 'FAILED: a change got the wrong "added by" or "changed by"';
  end if;
  update todo_helper.profiles set name = 'Renamed by B' where id = a;
  get diagnostics n = row_count;
  if n <> 0 then raise exception 'FAILED: a member can change another member''s name'; end if;
  begin
    insert into todo_helper.event_log (event_id, action) values (ev.id, 'added');
    raise exception 'FAILED: a member can write to the log';
  exception when insufficient_privilege then null;
  end;
  begin
    delete from todo_helper.event_log where event_id = ev.id;
    raise exception 'FAILED: a member can delete from the log';
  exception when insufficient_privilege then null;
  end;
  delete from todo_helper.events where id = ev.id;
  get diagnostics n = row_count;
  if n <> 1 then raise exception 'FAILED: a member can''t delete another member''s event'; end if;

  -- The log says who did what, and keeps the deleted event.
  select string_agg(action || ' by ' || case by_user when a then 'A' when b then 'B' else '?' end, ', ' order by id)
    into actions from todo_helper.event_log where event_id = ev.id;
  if actions is distinct from 'added by A, changed by B, deleted by B' then
    raise exception 'FAILED: the log says "%", not "added by A, changed by B, deleted by B"', actions;
  end if;
  select count(*) into n from todo_helper.event_log
    where event_id = ev.id and action = 'deleted' and before ->> 'title' = 'Check meeting, moved';
  if n <> 1 then raise exception 'FAILED: the log doesn''t keep the deleted event'; end if;

  -- Tasks: someone who isn't signed in sees and gives nothing.
  reset role;
  perform set_config('request.jwt.claims', '{"role": "anon"}', true);
  set local role anon;
  begin
    perform 1 from todo_helper.tasks;
    raise exception 'FAILED: someone who isn''t signed in can read tasks';
  exception when insufficient_privilege then null;
  end;
  begin
    insert into todo_helper.tasks (title, assignee, due) values ('x', b, current_date);
    raise exception 'FAILED: someone who isn''t signed in can give a task';
  exception when insufficient_privilege then null;
  end;

  -- A gives B a task and can't pretend B gave it.
  reset role;
  perform set_config('request.jwt.claims', json_build_object('sub', a, 'role', 'authenticated')::text, true);
  set local role authenticated;
  insert into todo_helper.tasks (title, assignee, due, created_by, updated_by)
    values ('Check task', b, current_date + 3, b, b) returning id, created_by, updated_by into tk, by_a, by_b;
  if by_a is distinct from a or by_b is distinct from a then
    raise exception 'FAILED: the app could set who gave a task';
  end if;

  -- B moves the deadline and marks it done; A deletes it. Nobody writes the log.
  reset role;
  perform set_config('request.jwt.claims', json_build_object('sub', b, 'role', 'authenticated')::text, true);
  set local role authenticated;
  update todo_helper.tasks set due = current_date + 5, done = true where id = tk;
  get diagnostics n = row_count;
  if n <> 1 then raise exception 'FAILED: the assignee can''t change a task'; end if;
  begin
    insert into todo_helper.task_log (task_id, action) values (tk, 'added');
    raise exception 'FAILED: a member can write to the task log';
  exception when insufficient_privilege then null;
  end;
  reset role;
  perform set_config('request.jwt.claims', json_build_object('sub', a, 'role', 'authenticated')::text, true);
  set local role authenticated;
  delete from todo_helper.tasks where id = tk;
  get diagnostics n = row_count;
  if n <> 1 then raise exception 'FAILED: a member can''t delete a task'; end if;

  select string_agg(action || ' by ' || case by_user when a then 'A' when b then 'B' else '?' end, ', ' order by id)
    into actions from todo_helper.task_log where task_id = tk;
  if actions is distinct from 'added by A, changed by B, deleted by A' then
    raise exception 'FAILED: the task log says "%", not "added by A, changed by B, deleted by A"', actions;
  end if;

  -- Clean up as the owner.
  reset role;
  delete from todo_helper.event_log where event_id = ev.id;
  delete from todo_helper.task_log where task_id = tk;
  delete from auth.users where id in (a, b);
end $$;

select 'All checks passed' as result;
