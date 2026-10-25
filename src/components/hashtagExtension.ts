import { Extension } from '@tiptap/core'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import { Decoration, DecorationSet } from '@tiptap/pm/view'
import { tagColor } from '../utils/tags'

// A tag is `#word` at the start of a line or after whitespace — never
// inside a word, and never a markdown heading (`# ` has a space).
export const HASHTAG = /(?:^|\s)(#[A-Za-z0-9][\w-]*)/g

/**
 * Highlights `#tags` inline (Apple Notes style) without touching the
 * document — decorations are computed from the text, so the stored
 * markdown stays plain `#word` and existing notes light up on open.
 * Tags are parsed from the markdown body (see extractTags), which
 * skips code spans/fences; decorations mirror that here.
 */
export const Hashtag = Extension.create({
  name: 'hashtag',
  addProseMirrorPlugins() {
    return [
      new Plugin({
        key: new PluginKey('hashtagDecorations'),
        props: {
          decorations(state) {
            const decorations: Decoration[] = []
            state.doc.descendants((node, pos, parent) => {
              if (node.type.name === 'codeBlock') return false
              if (!node.isText || !node.text) return
              if (node.marks.some((m) => m.type.name === 'code')) return
              if (parent?.type.name === 'codeBlock') return
              for (const m of node.text.matchAll(HASHTAG)) {
                const name = m[1].slice(1)
                const offset = m[0].indexOf('#')
                const from = pos + (m.index ?? 0) + offset
                decorations.push(
                  Decoration.inline(from, from + m[1].length, {
                    class: 'hashtag',
                    style: `--tag-color: ${tagColor(name)}`,
                  }),
                )
              }
            })
            return DecorationSet.create(state.doc, decorations)
          },
        },
      }),
    ]
  },
})
