/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_EXTRACTION_API_URL?: string;
  readonly VITE_REOWN_PROJECT_ID?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
