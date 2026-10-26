<script setup lang="ts">
// The Quick Note body is the same rich editor as Notes — markdown
// shortcuts, checklists, code blocks — just without the note chrome.
import { EditorContent, useEditor } from '@tiptap/vue-3'
import type { Editor } from '@tiptap/core'
import { noteExtensions } from './noteExtensions'

const props = defineProps<{ initial: string }>()
const emit = defineEmits<{ change: [markdown: string] }>()

// Mounted fresh each time the dialog opens, so `initial` is always the
// cleared body and the editor never outlives the modal.
const editor = useEditor({
  content: props.initial,
  extensions: noteExtensions('Write something… #tag to organize'),
  onUpdate: ({ editor }) => emit('change', markdownOf(editor)),
})

function markdownOf(e: Editor): string {
  const md = (e.storage as unknown as { markdown?: { getMarkdown(): string } }).markdown
  return md ? md.getMarkdown() : e.getText()
}

function focus() {
  editor.value?.commands.focus('end')
}

defineExpose({ focus })
</script>

<template>
  <EditorContent :editor="editor" class="note-prose quick-prose" aria-label="Note body" />
</template>

<style scoped>
.quick-prose :deep(.tiptap) {
  min-height: 140px;
}
</style>
