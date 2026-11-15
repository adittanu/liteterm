-- LiteTerm shared team vaults, end-to-end encrypted.
--
-- A team vault holds the same encrypted items as a personal vault, readable
-- by every member. The server never sees a team key: each member's copy is
-- sealed to that member's account key (X25519) by another member's app. What
-- the server does see: who is in which team, their roles and emails, the
-- team names, item counts, sizes and when they change.
--
-- Every write goes through the functions below, which check the caller's role
-- first. Tables are readable by members only (row level security).
--
-- Apply: supabase db query --linked -f supabase/migrations/20261116000000_team_vaults.sql
-- (or paste it in the dashboard SQL editor). Tests: supabase/tests/team_vaults_test.sql.

-- ---------------------------------------------------------------------------
-- account_keys: one key pair per account. The private half is sealed with the
-- account's personal vault key, so only that account's computers can open it.
-- ---------------------------------------------------------------------------
create table public.account_keys (
  user_id             uuid primary key references auth.users (id) on delete cascade,
  public_key          text not null check (length(public_key) between 40 and 64),
  private_key_sealed  text not null check (length(private_key_sealed) < 1024),
  created_at          timestamptz not null default now()
);

create table public.teams (
  id          uuid primary key default gen_random_uuid(),
  name        text not null check (length(btrim(name)) between 1 and 80),
  -- Per-team change counter; each pushed change takes the next value
  rev         bigint not null default 0,
  -- Which team key is current. Removing a member starts a new one.
  epoch       integer not null default 1,
  created_by  uuid references auth.users (id) on delete set null,
  created_at  timestamptz not null default now()
);

create table public.team_members (
  team_id   uuid not null references public.teams (id) on delete cascade,
  user_id   uuid not null references auth.users (id) on delete cascade,
  role      text not null check (role in ('owner', 'admin', 'editor', 'viewer')),
  -- Shown to the other members
  email     text not null,
  added_by  uuid references auth.users (id) on delete set null,
  added_at  timestamptz not null default now(),
  primary key (team_id, user_id)
);

create index team_members_user on public.team_members (user_id);

-- Each member's sealed copy of a team key, per epoch. Older epochs stay until
-- no item is encrypted with them any more.
create table public.team_keys (
  team_id      uuid not null,
  user_id      uuid not null,
  epoch        integer not null,
  wrapped_key  text not null check (length(wrapped_key) < 1024),
  primary key (team_id, user_id, epoch),
  foreign key (team_id, user_id) references public.team_members (team_id, user_id) on delete cascade
);

create table public.team_invites (
  id                   uuid primary key default gen_random_uuid(),
  team_id              uuid not null references public.teams (id) on delete cascade,
  -- Lower case; matched against the signed-in account's email
  email                text not null check (email = lower(email) and position('@' in email) > 1),
  role                 text not null check (role in ('admin', 'editor', 'viewer')),
  -- Copied at invite time: the invitee can't read the team or member rows yet
  team_name            text not null,
  invited_by_email     text not null,
  invited_by           uuid references auth.users (id) on delete set null,
  created_at           timestamptz not null default now(),
  expires_at           timestamptz not null default now() + interval '7 days',
  -- Set when the invitee accepts; the key the admitting member seals to
  accepted_by          uuid references auth.users (id) on delete cascade,
  accepted_public_key  text,
  accepted_at          timestamptz,
  unique (team_id, email)
);

create index team_invites_email on public.team_invites (email);

create table public.team_items (
  team_id      uuid not null references public.teams (id) on delete cascade,
  id           uuid not null,
  data         text,
  deleted      boolean not null default false,
  modified_at  bigint not null,
  rev          bigint not null,
  -- The team key epoch `data` is encrypted with
  epoch        integer not null,
  primary key (team_id, id),
  constraint data_present check (deleted or data is not null),
  constraint data_size check (octet_length(data) <= 256 * 1024)
);

create index team_items_rev on public.team_items (team_id, rev);
create index team_items_epoch on public.team_items (team_id, epoch);

-- ---------------------------------------------------------------------------
-- Helpers
-- ---------------------------------------------------------------------------

-- The caller's role in a team, or null
create function public.team_role(t uuid)
returns text
language sql
stable
security definer
set search_path = ''
as $$
  select m.role from public.team_members m where m.team_id = t and m.user_id = auth.uid()
$$;

-- Whether the caller shares a team with `other` (to read their public key)
create function public.shares_team(other uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select exists (
    select 1 from public.team_members a
    join public.team_members b on b.team_id = a.team_id
    where a.user_id = auth.uid() and b.user_id = other
  )
$$;

create function public.team_require(t uuid, allowed text[])
returns text
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  r text;
begin
  if auth.uid() is null then
    raise exception 'not signed in' using errcode = '28000';
  end if;
  r := public.team_role(t);
  if r is null then
    raise exception 'not a member of this team' using errcode = '42501';
  end if;
  if not (r = any (allowed)) then
    raise exception 'your role in this team can''t do that' using errcode = '42501';
  end if;
  return r;
end;
$$;

-- Checks that `wrapped` ([{"user_id": uuid, "wrapped_key": text}]) has
-- exactly one entry per member of the team
create function public.team_keys_cover_members(t uuid, wrapped jsonb)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select jsonb_typeof(wrapped) = 'array'
     and (select count(distinct (w->>'user_id')) from jsonb_array_elements(wrapped) w) = jsonb_array_length(wrapped)
     and (select array_agg(m.user_id order by m.user_id) from public.team_members m where m.team_id = t)
       = (select array_agg((w->>'user_id')::uuid order by (w->>'user_id')::uuid) from jsonb_array_elements(wrapped) w)
$$;

-- ---------------------------------------------------------------------------
-- Account keys
-- ---------------------------------------------------------------------------

-- Sets the caller's key pair once. A second call keeps the first key: every
-- team key already sealed to it would be lost otherwise.
create function public.put_account_key(public_key text, private_key_sealed text)
returns void
language plpgsql
security definer
set search_path = ''
as $$
begin
  if auth.uid() is null then
    raise exception 'not signed in' using errcode = '28000';
  end if;
  insert into public.account_keys (user_id, public_key, private_key_sealed)
  values (auth.uid(), put_account_key.public_key, put_account_key.private_key_sealed)
  on conflict (user_id) do nothing;
end;
$$;

-- ---------------------------------------------------------------------------
-- Teams and members
-- ---------------------------------------------------------------------------

create function public.create_team(name text, wrapped_key text)
returns uuid
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid  uuid := auth.uid();
  tid  uuid;
  mail text;
begin
  if uid is null then
    raise exception 'not signed in' using errcode = '28000';
  end if;
  if not exists (select 1 from public.account_keys k where k.user_id = uid) then
    raise exception 'set up sync with a master password first' using errcode = 'P0002';
  end if;
  if (select count(*) from public.team_members m where m.user_id = uid and m.role = 'owner') >= 20 then
    raise exception 'you own 20 teams already' using errcode = '54000';
  end if;
  select lower(u.email) into mail from auth.users u where u.id = uid;
  insert into public.teams (name, created_by) values (btrim(create_team.name), uid) returning id into tid;
  insert into public.team_members (team_id, user_id, role, email, added_by)
  values (tid, uid, 'owner', coalesce(mail, ''), uid);
  insert into public.team_keys (team_id, user_id, epoch, wrapped_key)
  values (tid, uid, 1, create_team.wrapped_key);
  return tid;
end;
$$;

create function public.rename_team(team_id uuid, name text)
returns void
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform public.team_require(rename_team.team_id, array['owner', 'admin']);
  update public.teams set name = btrim(rename_team.name) where id = rename_team.team_id;
end;
$$;

create function public.delete_team(team_id uuid)
returns void
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform public.team_require(delete_team.team_id, array['owner']);
  delete from public.teams where id = delete_team.team_id;
end;
$$;

-- Owners and admins invite; only an owner can invite an admin. Inviting the
-- same address again renews the invite.
create function public.invite_member(team_id uuid, email text, role text)
returns uuid
language plpgsql
security definer
set search_path = ''
as $$
declare
  caller text;
  mail   text := lower(btrim(invite_member.email));
  iid    uuid;
begin
  caller := public.team_require(invite_member.team_id, array['owner', 'admin']);
  if invite_member.role = 'admin' and caller <> 'owner' then
    raise exception 'only an owner can invite an admin' using errcode = '42501';
  end if;
  if exists (select 1 from public.team_members m where m.team_id = invite_member.team_id and m.email = mail) then
    raise exception 'that person is in the team already' using errcode = '23505';
  end if;
  if (select count(*) from public.team_members m where m.team_id = invite_member.team_id)
     + (select count(*) from public.team_invites i
        where i.team_id = invite_member.team_id and i.email <> mail and i.expires_at > now()) >= 50 then
    raise exception 'a team has at most 50 members' using errcode = '54000';
  end if;
  insert into public.team_invites (team_id, email, role, team_name, invited_by_email, invited_by)
  values (invite_member.team_id, mail, invite_member.role,
          (select t.name from public.teams t where t.id = invite_member.team_id),
          (select m.email from public.team_members m
            where m.team_id = invite_member.team_id and m.user_id = auth.uid()),
          auth.uid())
  on conflict on constraint team_invites_team_id_email_key do update
    set role = excluded.role, team_name = excluded.team_name, invited_by_email = excluded.invited_by_email,
        invited_by = excluded.invited_by, created_at = now(),
        expires_at = now() + interval '7 days',
        accepted_by = null, accepted_public_key = null, accepted_at = null
  returning id into iid;
  return iid;
end;
$$;

-- The invited account says yes; its public key is attached for the admitting
-- member to seal the team key to (after comparing safety codes)
create function public.accept_invite(invite_id uuid)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := auth.uid();
  pk  text;
begin
  if uid is null then
    raise exception 'not signed in' using errcode = '28000';
  end if;
  select k.public_key into pk from public.account_keys k where k.user_id = uid;
  if pk is null then
    raise exception 'set up sync with a master password first' using errcode = 'P0002';
  end if;
  update public.team_invites i
     set accepted_by = uid, accepted_public_key = pk, accepted_at = now()
   where i.id = accept_invite.invite_id
     and i.email = lower(auth.jwt() ->> 'email')
     and i.expires_at > now();
  if not found then
    raise exception 'this invite has expired or is not for this account' using errcode = 'P0002';
  end if;
end;
$$;

-- The invitee says no, or an owner or admin withdraws the invite
create function public.cancel_invite(invite_id uuid)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  tid  uuid;
  mail text;
begin
  select i.team_id, i.email into tid, mail from public.team_invites i where i.id = cancel_invite.invite_id;
  if tid is null then
    return;
  end if;
  if mail <> lower(coalesce(auth.jwt() ->> 'email', '')) then
    perform public.team_require(tid, array['owner', 'admin']);
  end if;
  delete from public.team_invites where id = cancel_invite.invite_id;
end;
$$;

-- An owner or admin admits someone who accepted, with the current team key
-- sealed to the key they accepted with. Not while a key change is still
-- re-encrypting items: the new member couldn't read the old ones.
create function public.admit_member(invite_id uuid, epoch integer, wrapped_key text)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  inv  public.team_invites%rowtype;
  cur  integer;
  mail text;
begin
  select * into inv from public.team_invites i where i.id = admit_member.invite_id;
  if inv.id is null or inv.accepted_by is null then
    raise exception 'that invite has not been accepted' using errcode = 'P0002';
  end if;
  perform public.team_require(inv.team_id, array['owner', 'admin']);
  select t.epoch into cur from public.teams t where t.id = inv.team_id for update;
  if admit_member.epoch <> cur then
    raise exception 'the team key changed; try again' using errcode = '40001';
  end if;
  if exists (select 1 from public.team_items it where it.team_id = inv.team_id and it.epoch < cur) then
    raise exception 'the team key is still being changed; try again in a moment' using errcode = '40001';
  end if;
  -- The key must not have changed since it was shown with the safety code
  if inv.accepted_public_key is distinct from
     (select k.public_key from public.account_keys k where k.user_id = inv.accepted_by) then
    raise exception 'that person''s key changed; ask them to accept again' using errcode = '40001';
  end if;
  select lower(u.email) into mail from auth.users u where u.id = inv.accepted_by;
  insert into public.team_members (team_id, user_id, role, email, added_by)
  values (inv.team_id, inv.accepted_by, inv.role, coalesce(mail, inv.email), auth.uid());
  insert into public.team_keys (team_id, user_id, epoch, wrapped_key)
  values (inv.team_id, inv.accepted_by, cur, admit_member.wrapped_key);
  delete from public.team_invites where id = inv.id;
end;
$$;

-- Owners change roles. A team keeps at least one owner.
create function public.set_member_role(team_id uuid, user_id uuid, role text)
returns void
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform public.team_require(set_member_role.team_id, array['owner']);
  if set_member_role.role not in ('owner', 'admin', 'editor', 'viewer') then
    raise exception 'unknown role' using errcode = '22023';
  end if;
  if set_member_role.role <> 'owner'
     and (select m.role from public.team_members m
          where m.team_id = set_member_role.team_id and m.user_id = set_member_role.user_id) = 'owner'
     and (select count(*) from public.team_members m
          where m.team_id = set_member_role.team_id and m.role = 'owner') = 1 then
    raise exception 'a team needs an owner' using errcode = '23514';
  end if;
  update public.team_members m set role = set_member_role.role
   where m.team_id = set_member_role.team_id and m.user_id = set_member_role.user_id;
end;
$$;

-- Starts a new epoch: `wrapped` holds the new team key sealed to every
-- remaining member. Older keys stay until rewrite_team_items has moved every
-- item to the new one.
create function public.team_new_epoch(t uuid, new_epoch integer, wrapped jsonb)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  cur integer;
begin
  select tm.epoch into cur from public.teams tm where tm.id = t for update;
  if new_epoch <> cur + 1 then
    raise exception 'the team key changed; try again' using errcode = '40001';
  end if;
  if not public.team_keys_cover_members(t, wrapped) then
    raise exception 'the new key must be sealed to every member' using errcode = '22023';
  end if;
  update public.teams set epoch = new_epoch where id = t;
  insert into public.team_keys (team_id, user_id, epoch, wrapped_key)
  select t, (w->>'user_id')::uuid, new_epoch, w->>'wrapped_key' from jsonb_array_elements(wrapped) w;
end;
$$;

-- Removes a member and changes the team key in one go, so nothing written
-- afterwards uses a key the removed member had
create function public.remove_member(team_id uuid, user_id uuid, new_epoch integer, wrapped jsonb)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  caller text;
  target text;
begin
  caller := public.team_require(remove_member.team_id, array['owner', 'admin']);
  if remove_member.user_id = auth.uid() then
    raise exception 'use leave_team to leave' using errcode = '22023';
  end if;
  select m.role into target from public.team_members m
   where m.team_id = remove_member.team_id and m.user_id = remove_member.user_id;
  if target is null then
    raise exception 'not a member of this team' using errcode = 'P0002';
  end if;
  if target in ('owner', 'admin') and caller <> 'owner' then
    raise exception 'only an owner can remove an owner or admin' using errcode = '42501';
  end if;
  delete from public.team_members m where m.team_id = remove_member.team_id and m.user_id = remove_member.user_id;
  perform public.team_new_epoch(remove_member.team_id, remove_member.new_epoch, remove_member.wrapped);
end;
$$;

-- Leaving changes the key too, for the same reason
create function public.leave_team(team_id uuid, new_epoch integer, wrapped jsonb)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  me text;
begin
  me := public.team_require(leave_team.team_id, array['owner', 'admin', 'editor', 'viewer']);
  if me = 'owner' and (select count(*) from public.team_members m
                       where m.team_id = leave_team.team_id and m.role = 'owner') = 1 then
    raise exception 'make someone else an owner first, or delete the team' using errcode = '23514';
  end if;
  delete from public.team_members m where m.team_id = leave_team.team_id and m.user_id = auth.uid();
  perform public.team_new_epoch(leave_team.team_id, leave_team.new_epoch, leave_team.wrapped);
end;
$$;

-- ---------------------------------------------------------------------------
-- Items
-- ---------------------------------------------------------------------------

-- As push_items, per team. Items must be encrypted with the current epoch:
-- a member still on the old key is told to refresh first.
-- items: [{"id": uuid, "data": text|null, "deleted": bool, "modified_at": ms}]
create function public.push_team_items(team_id uuid, epoch integer, items jsonb)
returns bigint
language plpgsql
security definer
set search_path = ''
as $$
declare
  cur_rev   bigint;
  cur_epoch integer;
  item      jsonb;
  is_del    boolean;
begin
  perform public.team_require(push_team_items.team_id, array['owner', 'admin', 'editor']);
  if jsonb_typeof(items) is distinct from 'array' or jsonb_array_length(items) > 500 then
    raise exception 'items must be an array of at most 500 entries' using errcode = '22023';
  end if;
  select t.rev, t.epoch into cur_rev, cur_epoch from public.teams t where t.id = push_team_items.team_id for update;
  if push_team_items.epoch <> cur_epoch then
    raise exception 'the team key changed; sync again' using errcode = '40001';
  end if;

  for item in select * from jsonb_array_elements(items) loop
    is_del := coalesce((item->>'deleted')::boolean, false);
    insert into public.team_items as v (team_id, id, data, deleted, modified_at, rev, epoch)
    values (
      push_team_items.team_id,
      (item->>'id')::uuid,
      case when is_del then null else item->>'data' end,
      is_del,
      (item->>'modified_at')::bigint,
      cur_rev + 1,
      cur_epoch
    )
    on conflict on constraint team_items_pkey do update
      set data        = excluded.data,
          deleted     = excluded.deleted,
          modified_at = excluded.modified_at,
          rev         = excluded.rev,
          epoch       = excluded.epoch
      where v.modified_at < excluded.modified_at;
    if found then
      cur_rev := cur_rev + 1;
    end if;
  end loop;

  if (select count(*) from public.team_items it
      where it.team_id = push_team_items.team_id and not it.deleted) > 5000 then
    raise exception 'a team vault holds at most 5000 items' using errcode = '54000';
  end if;
  update public.teams set rev = cur_rev where id = push_team_items.team_id;
  return cur_rev;
end;
$$;

-- After a key change: items re-encrypted with the current key replace their
-- old-key copies, unless someone edited them meanwhile. The content is the
-- same, so rev stays and nobody pulls them again. Once no item uses an older
-- key, those keys are deleted. Returns how many items still use one.
-- items: [{"id": uuid, "data": text, "modified_at": ms}]
create function public.rewrite_team_items(team_id uuid, epoch integer, items jsonb)
returns bigint
language plpgsql
security definer
set search_path = ''
as $$
declare
  cur_epoch integer;
  left_over bigint;
begin
  perform public.team_require(rewrite_team_items.team_id, array['owner', 'admin']);
  if jsonb_typeof(items) is distinct from 'array' or jsonb_array_length(items) > 500 then
    raise exception 'items must be an array of at most 500 entries' using errcode = '22023';
  end if;
  select t.epoch into cur_epoch from public.teams t where t.id = rewrite_team_items.team_id for update;
  if rewrite_team_items.epoch <> cur_epoch then
    raise exception 'the team key changed; sync again' using errcode = '40001';
  end if;

  update public.team_items it
     set data = w->>'data', epoch = cur_epoch
    from jsonb_array_elements(items) w
   where it.team_id = rewrite_team_items.team_id
     and it.id = (w->>'id')::uuid
     and it.epoch < cur_epoch
     and not it.deleted
     and it.modified_at = (w->>'modified_at')::bigint;
  -- A tombstone has nothing to re-encrypt
  update public.team_items it set epoch = cur_epoch
   where it.team_id = rewrite_team_items.team_id and it.deleted and it.epoch < cur_epoch;

  select count(*) into left_over from public.team_items it
   where it.team_id = rewrite_team_items.team_id and it.epoch < cur_epoch;
  if left_over = 0 then
    delete from public.team_keys k where k.team_id = rewrite_team_items.team_id and k.epoch < cur_epoch;
  end if;
  return left_over;
end;
$$;

-- ---------------------------------------------------------------------------
-- Access: members read their teams; writes only through the functions above
-- ---------------------------------------------------------------------------
alter table public.account_keys enable row level security;
alter table public.teams        enable row level security;
alter table public.team_members enable row level security;
alter table public.team_keys    enable row level security;
alter table public.team_invites enable row level security;
alter table public.team_items   enable row level security;

create policy "account keys: own, or a teammate's" on public.account_keys
  for select to authenticated
  using (user_id = (select auth.uid()) or public.shares_team(user_id));
create policy "teams: members" on public.teams
  for select to authenticated using (public.team_role(id) is not null);
create policy "team members: members" on public.team_members
  for select to authenticated using (public.team_role(team_id) is not null);
create policy "team keys: own copies" on public.team_keys
  for select to authenticated using (user_id = (select auth.uid()));
create policy "team invites: admins, or the invitee" on public.team_invites
  for select to authenticated
  using (public.team_role(team_id) in ('owner', 'admin') or email = lower((select auth.jwt()) ->> 'email'));
create policy "team items: members" on public.team_items
  for select to authenticated using (public.team_role(team_id) is not null);

revoke all on public.account_keys, public.teams, public.team_members, public.team_keys,
  public.team_invites, public.team_items from anon, authenticated;
grant select on public.account_keys, public.teams, public.team_members, public.team_keys,
  public.team_invites, public.team_items to authenticated;

revoke all on function
  public.team_role(uuid), public.shares_team(uuid), public.team_require(uuid, text[]),
  public.team_keys_cover_members(uuid, jsonb), public.team_new_epoch(uuid, integer, jsonb),
  public.put_account_key(text, text), public.create_team(text, text), public.rename_team(uuid, text),
  public.delete_team(uuid), public.invite_member(uuid, text, text), public.accept_invite(uuid),
  public.cancel_invite(uuid), public.admit_member(uuid, integer, text),
  public.set_member_role(uuid, uuid, text), public.remove_member(uuid, uuid, integer, jsonb),
  public.leave_team(uuid, integer, jsonb), public.push_team_items(uuid, integer, jsonb),
  public.rewrite_team_items(uuid, integer, jsonb)
-- Supabase grants execute to authenticated by default; the helpers must not be
-- callable on their own
from public, anon, authenticated;

-- Used by the policies
grant execute on function public.team_role(uuid), public.shares_team(uuid) to authenticated;
-- What the app calls
grant execute on function
  public.put_account_key(text, text), public.create_team(text, text), public.rename_team(uuid, text),
  public.delete_team(uuid), public.invite_member(uuid, text, text), public.accept_invite(uuid),
  public.cancel_invite(uuid), public.admit_member(uuid, integer, text),
  public.set_member_role(uuid, uuid, text), public.remove_member(uuid, uuid, integer, jsonb),
  public.leave_team(uuid, integer, jsonb), public.push_team_items(uuid, integer, jsonb),
  public.rewrite_team_items(uuid, integer, jsonb)
to authenticated;
