# Textree E2E (Playwright + WebView2 CDP)

실행 중인 Tauri 앱의 **실제 WebView2**에 CDP로 연결해 검증한다. mock IPC가 아니라
실제 파일시스템 백엔드를 거치므로 "파일시스템이 진실의 원천"이라는 핵심 가치를 그대로
검증한다. (CDP는 **Windows/WebView2 전용**.)

## 실행

두 개의 터미널이 필요하다.

**터미널 1 — 앱을 원격 디버깅 포트로 기동:**

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"
npm run tauri dev
```

**터미널 2 — E2E 실행:**

```bash
npm run test:e2e
```

## 구성

- `helpers.ts` — CDP 연결·앱 페이지 탐색, dev 브리지(`window.__textreeTest.loadVault`)로
  네이티브 폴더 다이얼로그 우회, 임시 볼트 fixture, 네이티브 DnD 디스패치.
- `smoke.spec.ts` — 볼트 열기·노트 선택(※ `sample-vault` 의존).
- `editing.spec.ts` — M2 편집·디바운스 자동저장·전환 flush.
- `sync.spec.ts` — M3 외부 동기화(재로드/생성/삭제/충돌 배너·해소).
- `structure.spec.ts` — M4 구조편집(생성/이름변경/삭제/DnD/＋하위/adopt/볼트전환 격리).
- `attachment.spec.ts` — M5 이미지 paste → assets/ 저장 + 링크.

## 참고

- `smoke.spec.ts`만 로컬 `sample-vault/`(.gitignore)에 의존한다. 나머지는 OS 임시
  디렉터리에 격리된 fixture 볼트를 만들고 정리하므로 자족적이다.
- dev 브리지는 `import.meta.env.DEV` 가드라 프로덕션 번들에서 트리쉐이크로 제거된다.
