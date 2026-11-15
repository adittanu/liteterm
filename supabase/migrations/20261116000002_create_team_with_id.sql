-- The creator's copy of the team key is sealed with the team id in its
-- associated data, so the app picks the id before it calls create_team.
--
-- Apply: supabase db query --linked -f supabase/migrations/20261116000002_create_team_with_id.sql

drop function public.create_team(text, text);

create function public.create_team(team_id uuid, name text, wrapped_key text)
returns uuid
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid  uuid := auth.uid();
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
  insert into public.teams (id, name, created_by) values (create_team.team_id, btrim(create_team.name), uid);
  insert into public.team_members (team_id, user_id, role, email, added_by)
  values (create_team.team_id, uid, 'owner', coalesce(mail, ''), uid);
  insert into public.team_keys (team_id, user_id, epoch, wrapped_key)
  values (create_team.team_id, uid, 1, create_team.wrapped_key);
  return create_team.team_id;
end;
$$;

revoke all on function public.create_team(uuid, text, text) from public, anon, authenticated;
grant execute on function public.create_team(uuid, text, text) to authenticated;
