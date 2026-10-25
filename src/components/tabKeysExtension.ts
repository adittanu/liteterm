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

function indentCode(view: EditorView) {
  const { from, to } = view.state.selection
  view.dispatch(view.state.tr.insertText('  ', from, to))
}

function outdentCode(view: EditorView) {
  const { from, empty, $from } = view.state.selection
  if (!empty) return
  const before = $from.parent.textBetween(0, $from.parentOffset, undefined, '\ufffc')
  const spaces = /^(?:[\s\S]*?)( {1,2})$/.exec(before)
  const n = spaces?.[1].length ?? 0
  if (n > 0) view.dispatch(view.state.tr.delete(from - n, from))
}
