#!/usr/bin/env node
// `eln` = elendirna의 thin alias. 실 launcher/바이너리 해석은 elendirna 패키지에 위임한다
// (N0097 r0010: eln 명령은 elendirna가 제공 + eln 이름 점유; v0.8: npx eln도 동작하도록 의존).
"use strict";
require("elendirna/bin/elendirna.js");
