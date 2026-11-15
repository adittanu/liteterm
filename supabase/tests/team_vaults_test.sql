-- Tests for 20261116000000_team_vaults.sql. They run on the real project,
-- inside a transaction that is rolled back, so nothing stays behind:
--
--   pwsh supabase/tests/run.ps1
--
-- (which sends: begin; <migration>; <this file>; rollback;)
--
-- Each check raises an exception when it fails, which aborts the run with
-- that message. Users are made up for the run and switch with as_user().

insert into auth.users (id, email, aud, role) values
  ('a11ce000-0000-4000-8000-000000000001', 'alice@test.liteterm', 'authenticated', 'authenticated'),
  ('b0b00000-0000-4000-8000-000000000002', 'bob@test.liteterm', 'authenticated', 'authenticated'),
  ('ca401000-0000-4000-8000-000000000003', 'carol@test.liteterm', 'authenticated', 'authenticated'),
  ('3a110000-0000-4000-8000-000000000004', 'mallory@test.liteterm', 'authenticated', 'authenticated');

create function pg_temp.as_user(name text) returns void language sql as $$
  select set_config('request.jwt.claims', json_build_object(
    'sub', case name
      when 'alice' then 'a11ce000-0000-4000-8000-000000000001'
      when 'bob' then 'b0b00000-0000-4000-8000-000000000002'
      when 'carol' then 'ca401000-0000-4000-8000-000000000003'
      else '3a110000-0000-4000-8000-000000000004' end,
    'email', upper(name) || '@test.liteterm',
    'role', 'authenticated')::text, true)
$$;

create function pg_temp.expect(ok boolean, what text) returns void language plpgsql as $$
begin
  if not coalesce(ok, false) then
    raise exception 'FAILED: %', what;
  end if;
end;
$$;

-- Runs `sql` and checks it fails with `state`
create function pg_temp.expect_error(sql text, state text, what text) returns void language plpgsql as $$
begin
  execute sql;
  raise exception 'FAILED (no error): %', what;
exception
  when others then
    if sqlstate = 'P0001' and sqlerrm like 'FAILED%' then
      raise;
    end if;
    if sqlstate <> state then
      raise exception 'FAILED (% %, wanted %): %', sqlstate, sqlerrm, state, what;
    end if;
end;
$$;

grant execute on function pg_temp.as_user(text), pg_temp.expect(boolean, text),
  pg_temp.expect_error(text, text, text) to authenticated;

set local role authenticated;

-- ---- account keys and creating a team ----
select pg_temp.as_user('alice');
select pg_temp.expect_error($$select public.create_team('7ea70000-0000-4000-8000-000000000001', 'Ops', 'wrapped-a1')$$, 'P0002',
  'a team needs an account key first');
select public.put_account_key('AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=', 'sealed-a');
select public.put_account_key('ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ=', 'sealed-other');
select pg_temp.expect((select public_key from public.account_keys
  where user_id = 'a11ce000-0000-4000-8000-000000000001') like 'AAAA%', 'the first account key stays');
select public.reseal_account_key('sealed-a2');
select pg_temp.expect((select public_key || ' ' || private_key_sealed from public.account_keys
  where user_id = 'a11ce000-0000-4000-8000-000000000001') = 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA= sealed-a2',
  'resealing changes only the sealed half');
select set_config('test.team', public.create_team('7ea70000-0000-4000-8000-000000000001', '  Ops  ', 'wrapped-a1')::text, true);
select pg_temp.expect((select name from public.teams) = 'Ops', 'the name is trimmed');
select pg_temp.expect((select role from public.team_members) = 'owner', 'the creator owns the team');

select pg_temp.as_user('bob');
select public.put_account_key('BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=', 'sealed-b');
select pg_temp.expect((select count(*) from public.teams) = 0, 'outsiders do not see the team');
select pg_temp.expect((select count(*) from public.account_keys) = 1, 'outsiders see only their own key');

-- ---- invite, accept, admit ----
select pg_temp.as_user('alice');
select set_config('test.invite_bob',
  public.invite_member(current_setting('test.team')::uuid, ' Bob@Test.LiteTerm ', 'editor')::text, true);

select pg_temp.as_user('mallory');
select pg_temp.expect((select count(*) from public.team_invites) = 0, 'others do not see the invite');
select pg_temp.expect_error(format($$select public.accept_invite(%L)$$, current_setting('test.invite_bob')),
  'P0002', 'an invite is accepted only by its address');

select pg_temp.as_user('bob');
select pg_temp.expect((select count(*) from public.team_invites) = 1, 'the invitee sees the invite');
select pg_temp.expect((select team_name || ' / ' || invited_by_email from public.team_invites) = 'Ops / alice@test.liteterm',
  'with the team name and who invited, though the team itself is hidden');
select public.accept_invite(current_setting('test.invite_bob')::uuid);
select pg_temp.expect((select count(*) from public.team_items) = 0, 'accepting alone gives no access');
select pg_temp.expect_error(format($$select public.admit_member(%L, 1, 'x')$$, current_setting('test.invite_bob')),
  '42501', 'the invitee can not admit themselves');

select pg_temp.as_user('alice');
select pg_temp.expect((select accepted_public_key from public.team_invites) like 'BBBB%',
  'the admitting member sees the key to seal to');
select pg_temp.expect_error(format($$select public.admit_member(%L, 2, 'x')$$, current_setting('test.invite_bob')),
  '40001', 'admitting with the wrong epoch');
select public.admit_member(current_setting('test.invite_bob')::uuid, 1, 'wrapped-b1');
select pg_temp.expect((select count(*) from public.team_members) = 2, 'bob is a member');
select pg_temp.expect((select count(*) from public.team_invites) = 0, 'the invite is used up');
select pg_temp.expect((select count(*) from public.account_keys) = 2, 'members see each other''s keys');

-- ---- items ----
select pg_temp.as_user('bob');
select pg_temp.expect((select count(*) from public.team_keys) = 1, 'a member sees only their own key copy');
select pg_temp.expect((select wrapped_key from public.team_keys) = 'wrapped-b1', 'it is theirs');
select pg_temp.expect(public.push_team_items(current_setting('test.team')::uuid, 1, $$[
  {"id": "11111111-0000-4000-8000-000000000001", "data": "host-1", "deleted": false, "modified_at": 100},
  {"id": "11111111-0000-4000-8000-000000000002", "data": "host-2", "deleted": false, "modified_at": 100}
]$$) = 2, 'an editor pushes');
select pg_temp.expect(public.push_team_items(current_setting('test.team')::uuid, 1, $$[
  {"id": "11111111-0000-4000-8000-000000000001", "data": "older", "deleted": false, "modified_at": 50}
]$$) = 2, 'an older edit is skipped');
select pg_temp.expect_error(format($$select public.push_team_items(%L, 2, '[]')$$, current_setting('test.team')),
  '40001', 'a push with the wrong epoch');
select pg_temp.expect_error(format($$select public.invite_member(%L, 'x@y.z', 'viewer')$$, current_setting('test.team')),
  '42501', 'an editor can not invite');

-- A viewer reads but can't write
select pg_temp.as_user('carol');
select public.put_account_key('CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC=', 'sealed-c');
select pg_temp.as_user('alice');
select set_config('test.invite_carol',
  public.invite_member(current_setting('test.team')::uuid, 'carol@test.liteterm', 'viewer')::text, true);
select pg_temp.as_user('carol');
select public.accept_invite(current_setting('test.invite_carol')::uuid);
select pg_temp.as_user('alice');
select public.admit_member(current_setting('test.invite_carol')::uuid, 1, 'wrapped-c1');
select pg_temp.as_user('carol');
select pg_temp.expect((select count(*) from public.team_items) = 2, 'a viewer reads the items');
select pg_temp.expect_error(format($$select public.push_team_items(%L, 1, '[]')$$, current_setting('test.team')),
  '42501', 'a viewer can not push');

select pg_temp.as_user('mallory');
select public.put_account_key('MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM=', 'sealed-m');
select pg_temp.expect((select count(*) from public.team_items) = 0, 'outsiders read no items');
select pg_temp.expect_error(format($$select public.push_team_items(%L, 1, '[]')$$, current_setting('test.team')),
  '42501', 'outsiders can not push');

-- ---- removing a member changes the key ----
select pg_temp.as_user('alice');
select pg_temp.expect_error(format($$select public.remove_member(%L, 'ca401000-0000-4000-8000-000000000003', 2,
  '[{"user_id": "a11ce000-0000-4000-8000-000000000001", "wrapped_key": "a2"}]')$$, current_setting('test.team')),
  '22023', 'the new key must reach every remaining member');
select pg_temp.expect_error(format($$select public.remove_member(%L, 'ca401000-0000-4000-8000-000000000003', 2,
  '[{"user_id": "a11ce000-0000-4000-8000-000000000001", "wrapped_key": "a2"},
    {"user_id": "b0b00000-0000-4000-8000-000000000002", "wrapped_key": "b2"},
    {"user_id": "ca401000-0000-4000-8000-000000000003", "wrapped_key": "c2"}]')$$, current_setting('test.team')),
  '22023', 'not to the removed member');
select public.remove_member(current_setting('test.team')::uuid, 'ca401000-0000-4000-8000-000000000003', 2,
  '[{"user_id": "a11ce000-0000-4000-8000-000000000001", "wrapped_key": "a2"},
    {"user_id": "b0b00000-0000-4000-8000-000000000002", "wrapped_key": "b2"}]');
select pg_temp.expect((select epoch from public.teams) = 2, 'a new epoch');
select pg_temp.expect((select count(*) from public.team_keys) = 2, 'old and new key copies, while items use the old');

select pg_temp.as_user('carol');
select pg_temp.expect((select count(*) from public.team_items) = 0, 'the removed member reads nothing');
select pg_temp.expect((select count(*) from public.team_keys) = 0, 'and has no key copies left');

-- Nobody joins while items still use the old key
select pg_temp.as_user('alice');
select set_config('test.invite_m',
  public.invite_member(current_setting('test.team')::uuid, 'mallory@test.liteterm', 'viewer')::text, true);
select pg_temp.as_user('mallory');
select public.accept_invite(current_setting('test.invite_m')::uuid);
select pg_temp.as_user('alice');
select pg_temp.expect_error(format($$select public.admit_member(%L, 2, 'm2')$$, current_setting('test.invite_m')),
  '40001', 'admitting during a key change');
select pg_temp.expect_error(format($$select public.push_team_items(%L, 1, '[]')$$, current_setting('test.team')),
  '40001', 'pushing with the old key');

-- Re-encrypting: an item edited meanwhile keeps its newer copy
select public.push_team_items(current_setting('test.team')::uuid, 2, $$[
  {"id": "11111111-0000-4000-8000-000000000002", "data": "host-2 edited", "deleted": false, "modified_at": 200}
]$$);
select pg_temp.expect(public.rewrite_team_items(current_setting('test.team')::uuid, 2, $$[
  {"id": "11111111-0000-4000-8000-000000000001", "data": "host-1 v2", "modified_at": 100},
  {"id": "11111111-0000-4000-8000-000000000002", "data": "host-2 v2", "modified_at": 100}
]$$) = 0, 'every item on the new key');
select pg_temp.expect((select data from public.team_items where id = '11111111-0000-4000-8000-000000000001') = 'host-1 v2',
  'the old copy is replaced');
select pg_temp.expect((select data from public.team_items where id = '11111111-0000-4000-8000-000000000002') = 'host-2 edited',
  'a newer edit is not overwritten');
select pg_temp.expect((select count(*) from public.team_keys) = 1, 'the old key copy is gone');
select pg_temp.expect((select rev from public.teams) = 3, 're-encrypting moves no rev');
select public.admit_member(current_setting('test.invite_m')::uuid, 2, 'm2');

-- ---- roles and leaving ----
select pg_temp.expect_error(format($$select public.leave_team(%L, 3, '[]')$$, current_setting('test.team')),
  '23514', 'the last owner can not leave');
select pg_temp.expect_error(format($$select public.set_member_role(%L, 'a11ce000-0000-4000-8000-000000000001', 'admin')$$,
  current_setting('test.team')), '23514', 'nor stop being an owner');
select public.set_member_role(current_setting('test.team')::uuid, 'b0b00000-0000-4000-8000-000000000002', 'owner');

select pg_temp.as_user('mallory');
select pg_temp.expect_error(format($$select public.delete_team(%L)$$, current_setting('test.team')),
  '42501', 'a viewer can not delete the team');
select public.leave_team(current_setting('test.team')::uuid, 3,
  '[{"user_id": "a11ce000-0000-4000-8000-000000000001", "wrapped_key": "a3"},
    {"user_id": "b0b00000-0000-4000-8000-000000000002", "wrapped_key": "b3"}]');
select pg_temp.expect((select count(*) from public.teams) = 0, 'after leaving the team is gone from view');

select pg_temp.as_user('bob');
select pg_temp.expect((select epoch from public.teams) = 3, 'leaving changed the key');
select public.delete_team(current_setting('test.team')::uuid);
select pg_temp.expect((select count(*) from public.teams) = 0, 'an owner deletes the team');

reset role;
select 'all team vault tests passed' as result;
