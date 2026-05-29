# eln

Alias package for [`elendirna`](https://www.npmjs.com/package/elendirna). Installing
`eln` pulls in `elendirna` and exposes the `eln` command, which runs the same
`elf` binary.

```sh
npx -y eln --help
# or
npm i -g eln
```

`npm i -g elendirna` already provides both `elendirna` and `eln` commands — this
package exists so `npx eln` / `npm i eln` resolve to the same tool. See the
[`elendirna` README](https://www.npmjs.com/package/elendirna) for details.
