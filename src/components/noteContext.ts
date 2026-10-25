import type { InjectionKey, Ref } from 'vue'

/** The host the open note is linked to — code block node views read it to
 *  pick where "Run" sends the command. */
export const NOTE_HOST_ID: InjectionKey<Ref<number | null>> = Symbol('noteHostId')
