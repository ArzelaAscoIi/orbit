# Social slides

Two 1080x1350 slides (LinkedIn portrait) about the work system around Orbit, in the slate theme.

| File | What |
|---|---|
| `cover.png` / `cover.html` | Agents drive. I navigate. |
| `how-it-works.png` / `how-it-works.html` | The loop: me, chief of staff, merkwerk and the knowledge graph, Orbit, the agents. |

Re-render a PNG after editing its HTML:

```sh
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --hide-scrollbars \
  --force-device-scale-factor=1 --virtual-time-budget=5000 --window-size=1080,1350 \
  --screenshot="$PWD/docs/social/cover.png" "file://$PWD/docs/social/cover.html"
```
