# Demo-Auth-Vault

> Elendirna vault — `elf` CLI로 관리되는 지식 저장소.

## 시작하기

```bash
elf entry new "아이디어 제목"
elf entry show N0001
elf entry edit N0001
elf revision add N0001 --delta "생각의 변화 내용"
elf link N0001 N0002
elf validate
```

## 인라인 cross-reference

note.md나 revision 본문에서 다른 entry를 참조할 때:
`→ see N####` 패턴을 사용하세요. `elf validate`가 dangling 여부를 자동 검사합니다.

예시:
```
이 아이디어는 그래프 탐색의 한계에서 출발합니다. → see N0001
```
