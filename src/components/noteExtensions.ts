import type { Extensions } from '@tiptap/core'
import StarterKit from '@tiptap/starter-kit'
import CodeBlock from '@tiptap/extension-code-block'
import Placeholder from '@tiptap/extension-placeholder'
import TaskList from '@tiptap/extension-task-list'
import TaskItem from '@tiptap/extension-task-item'
import TableRow from '@tiptap/extension-table-row'
import TableHeader from '@tiptap/extension-table-header'
import TableCell from '@tiptap/extension-table-cell'
import { VueNodeViewRenderer } from '@tiptap/vue-3'
import { Markdown } from 'tiptap-markdown'
import { Hashtag } from './hashtagExtension'
import { NoteImages } from './noteImages'
import { NoteTable } from './noteTable'
import { TabKeys } from './tabKeysExtension'
import CodeBlockView from './CodeBlockView.vue'

// Code blocks get Copy/Run actions through a Vue node view; same node
// name, so markdown storage and the other extensions see no difference
const RunnableCodeBlock = CodeBlock.extend({
  addNodeView() {
    return VueNodeViewRenderer(CodeBlockView)
  },
})

/** The one editor setup shared by Notes and Quick Note, so markdown
 *  shortcuts (`- `, `1. `, `[] `, `# `, `*x*`, …) behave identically. */
export function noteExtensions(placeholder: string): Extensions {
  return [
    StarterKit.configure({ heading: { levels: [1, 2, 3] }, codeBlock: false }),
    RunnableCodeBlock,
    Placeholder.configure({ placeholder }),
    TaskList,
    TaskItem.configure({ nested: true }),
    NoteTable.configure({ resizable: false }),
    TableRow,
    TableHeader,
    TableCell,
    // transformPastedText stays off: it runs plain pasted text through
    // markdown-it, which eats the backslash in `C:\Users\me\.ssh\config`,
    // `\\server\share` and `grep '\.log$'`, and turns `__init__.py` bold —
    // and notes are mostly pasted commands and paths. Typed markdown
    // shortcuts (`- `, `# `, `*x*`, …) still work.
    Markdown.configure({ html: false, breaks: true, transformPastedText: false }),
    Hashtag,
    NoteImages,
    // Last on purpose: its Tab handling must run after the list/task
    // indent shortcuts so it only sees keys they didn't consume
    TabKeys,
  ]
}
