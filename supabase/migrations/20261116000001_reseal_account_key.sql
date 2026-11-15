-- After a personal vault reset the account key's private half must be sealed
-- again, under the new vault key, by a computer that still holds it. The
-- public half (which every team key copy is sealed to) can't change.
--
-- Apply: supabase db query --linked -f supabase/migrations/20261116000001_reseal_account_key.sql

create function public.reseal_account_key(private_key_sealed text)
returns void
language plpgsql
security definer
set search_path = ''
as $$
begin
  if auth.uid() is null then
    raise exception 'not signed in' using errcode = '28000';
  end if;
  if length(reseal_account_key.private_key_sealed) >= 1024 then
    raise exception 'that key is too long' using errcode = '22023';
  end if;
  update public.account_keys k set private_key_sealed = reseal_account_key.private_key_sealed
   where k.user_id = auth.uid();
  if not found then
    raise exception 'this account has no account key yet' using errcode = 'P0002';
  end if;
end;
$$;

revoke all on function public.reseal_account_key(text) from public, anon, authenticated;
grant execute on function public.reseal_account_key(text) to authenticated;
