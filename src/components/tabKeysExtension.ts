import { Extension } from '@tiptap/core'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import type { ResolvedPos } from '@tiptap/pm/model'
import type { EditorView } from '@tiptap/pm/view'

// Owns the Tab key end to end (this plugin sees Tab before the
// schema-driven keymaps): list items sink/lift with the same commands
// the native shortcuts use, code blocks indent two spaces, and
// everywhere else Tab is swallowed so focus never escapes the editor.
export const TabKeys = Extension.create({
  name: 'tabKeys',
  addProseMirrorPlugins() {
    const editor = this.editor
    return [
      new Plugin({
        key: new PluginKey('tabKeys'),
        props: {
          handleKeyDown(view, event) {
            if (event.key !== 'Tab' || event.ctrlKey || event.metaKey || event.altKey) {
              return false
            }
            // Inside a table, Tab belongs to the Table extension (next cell, or a
            // new row at the end). This plugin runs first, so swallowing the key
            // here made table navigation by keyboard impossible.
            if (insideTable(view.state.selection.$from)) return false
            event.preventDefault()
            const { state } = view
            if (state.selection.$from.parent.type.name === 'codeBlock') {
              if (event.shiftKey) outdentCode(view)
              else indentCode(view)
              return true
            }
            const item = listItemAround(state.selection.$from)
            if (item) {
              const cmd = event.shiftKey ? editor.commands.liftListItem : editor.commands.sinkListItem
              cmd.call(editor.commands, item)
            }
            return true
          },
        },
      }),
    ]
  },
})

function listItemAround($from: ResolvedPos): string | null {
  for (let d = $from.depth; d > 0; d--) {
    const name = $from.node(d).type.name
    if (name === 'listItem' || name === 'taskItem') return name
  }
  return null
}

function insideTable($from: ResolvedPos): boolean {
  for (let d = $from.depth; d > 0; d--) {
    if ($from.node(d).type.name === 'table') return true
  }
  return false
}

function indentCode(view: EditorView) {
  const { state } = view
  const { from, to, empty, $from } = state.selection
  if (empty) {
    view.dispatch(state.tr.insertText('  ', from, to))
    return
  }
  // A selection: indent every line it touches. Replacing the selection with
  // two spaces — what this did — deleted the code that was selected.
  const blockStart = $from.start()
  const text = $from.parent.textContent
  const first = from - blockStart
  const last = Math.min(to - blockStart, text.length)
  const lineStarts: number[] = []
  let lineStart = 0
  for (;;) {
    const lineEnd = text.indexOf('\n', lineStart)
    const end = lineEnd === -1 ? text.length : lineEnd
    // a line is touched when the selection overlaps it (or ends right at its start)
    if (end >= first && lineStart <= last) lineStarts.push(lineStart)
    if (lineEnd === -1) break
    lineStart = lineEnd + 1
  }
  const tr = state.tr
  // Back to front, so earlier positions stay valid while inserting
  for (const offset of lineStarts.reverse()) tr.insertText('  ', blockStart + offset)
  view.dispatch(tr)
}

function outdentCode(view: EditorView) {
  const { from, empty, $from } = view.state.selection
  if (!empty) return
  const before = $from.parent.textBetween(0, $from.parentOffset, undefined, '\ufffc')
  const spaces = /^(?:[\s\S]*?)( {1,2})$/.exec(before)
  const n = spaces?.[1].length ?? 0
  if (n > 0) view.dispatch(view.state.tr.delete(from - n, from))
}
