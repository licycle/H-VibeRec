# Repository Guidelines & Architecture Principles

## 🏗️ Architecture Overview

### Core Design Philosophy
This is a **Tauri-based desktop application** with a **layered architecture** where:
- **React frontend** acts as the **application coordinator** - manages state, orchestrates business logic, and controls UI
- **Rust backend** provides **local system services** - audio processing, file operations, system permissions
- **Remote services** are **functionally equivalent** to local services - both act as service providers

### Service Call Strategy (Hybrid Approach)
The application uses a **hybrid service strategy** based on functionality requirements:

- **🔒 Through Rust (Security First)**:
  - File uploads/downloads (large files, progress tracking, resumption)
  - Authentication (secure credential management)
  - Data synchronization (offline queues, retry logic)
  - Sensitive operations (API keys, certificates)

- **🌐 Direct Frontend (Efficiency First)**:
  - Simple CRUD operations (quick data queries)
  - Real-time communication (WebSocket connections)
  - UI state synchronization
  - Third-party public APIs

### Key Architectural Principles
1. **Frontend as Coordinator**: React manages application state and orchestrates business flows
2. **Service Equivalence**: Local (Rust) and remote services provide equivalent functionality
3. **Interface Abstraction**: Services implement common interfaces for seamless switching
4. **Progressive Enhancement**: Offline-first design with optional cloud synchronization

## 📁 Project Structure & Module Organization

### Current Structure
- `src/` contains the React/TypeScript UI. `components/` holds reusable panes, `lib/` manages local persistence and Tauri bridges, and `types.ts` centralizes shared models.
- `src-tauri/` hosts the Rust backend; `src/audio/` captures and processes audio, `lib.rs` exposes commands, and `tauri.conf.json` drives bundling.
- `public/` serves static assets during development; `dist/` stores production builds; `recordings/` keeps user audio and should stay out of commits.
- Helper scripts live in functional subfolders under `scripts/`: `dev/`, `setup/`, `runtime/`, `package/`, `assets/`, and `debug/`.
- Branding assets live under `assets/branding/`; archived planning docs live under `docs/archive/`.

### Target Layered Structure (Migration Goal)
```
src/
├── api/                    # API Client Layer
│   ├── clients/            # Tauri IPC and HTTP clients
│   ├── adapters/           # Service adapters (audio, storage, sync)
│   └── index.ts            # API layer exports
├── services/               # Business Service Layer
│   ├── audio.service.ts    # Audio recording service
│   ├── meeting.service.ts  # Meeting management
│   ├── note.service.ts     # Note management
│   └── sync.service.ts     # Data synchronization
├── stores/                 # State Management Layer
│   ├── meeting.store.ts    # Meeting state (Zustand/Redux)
│   ├── recording.store.ts  # Recording state
│   └── app.store.ts        # Global app state
├── hooks/                  # React Integration Layer
│   ├── use-audio.ts        # Audio-related hooks
│   └── use-meetings.ts     # Meeting-related hooks
├── components/             # UI Component Layer
│   ├── ui/                 # Generic reusable components
│   ├── features/           # Feature-specific components
│   └── layout/             # Layout components
└── lib/                    # Utilities & Infrastructure
    ├── di-container.ts     # Dependency injection
    └── config.ts           # App configuration
```

## Build, Test, and Development Commands
- `npm install` or `./scripts/setup/install-dependencies.sh` installs dependencies for both web and Tauri tooling.
- `npm run dev` starts the Vite UI and ensures Vditor assets are copied for browser checks.
- `npm run tauri dev` launches the desktop app (requires Tauri prerequisites and the Rust toolchain).
- `npm run build` compiles TypeScript and outputs the production bundle to `dist/`.
- `npm run preview` serves the built UI for smoke testing.
- `npm run tauri build` (through the Tauri CLI) produces distributable binaries.

## 🎯 Service Layer Implementation Guidelines

### Service Interface Pattern
All services must implement consistent interfaces to enable seamless switching between local and remote implementations:

```typescript
// ✅ Proper service interface definition
interface AudioService {
  startRecording(options?: RecordingOptions): Promise<void>;
  stopRecording(): Promise<Recording>;
  getRecordingInfo(): Promise<RecordingInfo>;
}

// ✅ Multiple implementations
class TauriAudioService implements AudioService { /* Rust backend */ }
class WebAudioService implements AudioService { /* Browser API */ }
```

### Service Selection Strategy

| Service Type          | Implementation   | Reasoning                                |
| --------------------- | ---------------- | ---------------------------------------- |
| **Audio Recording**   | 🔒 Rust Required  | System permissions, real-time processing |
| **File Operations**   | 🔒 Rust Preferred | Large files, progress tracking           |
| **Authentication**    | 🔒 Rust Preferred | Secure token management                  |
| **Data Sync**         | 🔒 Rust Preferred | Offline queue, retry logic               |
| **Simple CRUD**       | 🌐 Frontend OK    | Fast iteration, standard patterns        |
| **Real-time Updates** | 🌐 Frontend OK    | WebSocket, instant UI feedback           |

### Dependency Injection Setup
```typescript
// lib/di-container.ts - Runtime service selection
const container = new Container();

if (window.__TAURI__) {
  container.bind<AudioService>('AudioService').to(TauriAudioService);
} else {
  container.bind<AudioService>('AudioService').to(WebAudioService);
}
```

## 🔧 Coding Style & Naming Conventions

### TypeScript/React Guidelines
- Follow the existing two-space indentation in TypeScript; prefer explicit types on exported helpers and React props.
- Use PascalCase for components (`MeetingSidebar`), camelCase for functions and state setters, and UPPER_SNAKE_CASE for constants.
- Keep hooks at the top level of components and encapsulate side effects in named helpers for clarity.

### Service Implementation Patterns
```typescript
// ✅ Proper service injection in components
const RecordingComponent: React.FC = () => {
  const audioService = useService<AudioService>('AudioService');
  const handleRecord = () => audioService.startRecording();
  return <button onClick={handleRecord}>Record</button>;
};

// ❌ Avoid direct API calls in components
const BadComponent: React.FC = () => {
  const handleRecord = () => invoke('start_recording'); // Don't do this
};
```

### Rust Backend Guidelines
- Run `cargo fmt` before committing Rust changes and mirror the module structure (`mod.rs`, `core.rs`, etc.) when expanding audio features.
- Use proper error handling: `Result<T, String>` for Tauri commands
- Add unit tests with `#[cfg(test)]` for new functionality

## 🧪 Testing Guidelines

### Manual Testing Checklist
Before any changes, verify these core workflows:
- [ ] **Recording Flow**: Start recording → Record audio → Stop → Save file
- [ ] **Sync Flow**: Upload to server → Download → Handle conflicts
- [ ] **Offline Mode**: Work without network → Sync when reconnected
- [ ] **Cross-platform**: Test on macOS, Windows, Linux (when applicable)

### Service Testing Strategy
```typescript
// ✅ Test service interfaces independently
describe('AudioService', () => {
  it('should start recording', async () => {
    const mockService = new MockAudioService();
    await expect(mockService.startRecording()).resolves.not.toThrow();
  });
});
```

### Current Testing Approach
- A JavaScript test harness is not yet configured; until Vitest or similar is added, rely on manual regression passes via `npm run dev` and Tauri smoke tests.
- For Rust modules, add `#[cfg(test)]` units alongside implementations and run `cargo test` within `src-tauri`.
- Document manual QA steps in PRs (scenarios exercised, platforms, mic setups) so reviewers can reproduce.

### Future Testing Goals
- **Frontend**: Vitest + React Testing Library for component testing
- **Services**: Mock implementations for unit testing
- **E2E**: Playwright for complete workflow validation

## 📝 Commit & Pull Request Guidelines

### Commit Message Format
```
type(scope): brief description

- Detailed explanation if needed
- Reference issues: #123
- Breaking changes noted
```

### PR Requirements for Architecture Compliance
1. **Layer Adherence**: Changes follow the layered architecture principles
2. **Service Abstraction**: New features use service interfaces, not direct API calls
3. **Type Safety**: Full TypeScript coverage with proper error handling
4. **Manual Testing**: Verify core workflows still function
5. **Documentation**: Update relevant architectural documentation

### Code Review Focus Areas
- **❌ Layer Violations**: Components directly calling `invoke()` or Tauri APIs
- **❌ Missing Abstractions**: New functionality not using service patterns
- **✅ Proper Separation**: Clear boundaries between UI, services, and API layers
- **✅ Service Patterns**: Consistent interface implementations

### Legacy Code Improvement
- When touching existing code, gradually refactor towards service abstraction
- Don't break existing functionality during architectural improvements
- Document any architectural debt for future cleanup

### Commit Guidelines
- Write concise, present-tense commit subjects (e.g., `refactor: extract audio service interface`); include Chinese context after the English verb if it adds clarity.
- Group related changes per commit and reference issue IDs or meeting notes in the body when applicable.
- Pull requests should describe motivation, highlight risky areas (audio pipeline, persistence), attach screenshots or recordings for UI/UX tweaks, and list verification commands (`npm run build`, `cargo test`).
- Confirm no sensitive files (real recordings, auth tokens) are staged before requesting review.

---

## 🤖 AI Development Guidelines

### For Claude/AI Assistants Modifying This Codebase

#### Understanding the Architecture
- **Frontend Role**: Application coordinator and state manager, NOT a service provider
- **Backend Role**: Local system service provider, equivalent to remote APIs
- **Service Pattern**: Always use abstracted interfaces, never direct API calls in components

#### Before Making Changes
1. **Identify the Layer**: Determine which architectural layer your changes affect
2. **Check Service Abstractions**: Verify if service interfaces exist for your use case
3. **Follow Hybrid Strategy**: Use the service selection guide (🔒 Rust vs 🌐 Frontend)
4. **Maintain Compatibility**: Don't break existing workflows during refactoring

#### Common Anti-Patterns to Avoid
```typescript
// ❌ Don't do this - Direct API coupling in components
const BadComponent: React.FC = () => {
  const handleUpload = () => {
    invoke('upload_file', { path }); // Violates abstraction
  };
};

// ❌ Don't do this - Mixed concerns in services
class BadService {
  async uploadFile(file: File) {
    // UI logic mixed with business logic
    setLoading(true);
    const result = await invoke('upload_file', { path: file.path });
    setLoading(false);
    return result;
  }
}
```

#### Proper Patterns to Follow
```typescript
// ✅ Do this - Service abstraction
const GoodComponent: React.FC = () => {
  const uploadService = useUploadService();
  const [isLoading, setIsLoading] = useState(false);

  const handleUpload = async () => {
    setIsLoading(true);
    try {
      await uploadService.uploadFile(file);
    } finally {
      setIsLoading(false);
    }
  };
};

// ✅ Do this - Clean service implementation
interface UploadService {
  uploadFile(file: File): Promise<UploadResult>;
}

class TauriUploadService implements UploadService {
  async uploadFile(file: File) {
    return invoke('upload_file_chunked', {
      filePath: file.path,
      serverUrl: this.config.serverUrl,
      authToken: this.config.authToken
    });
  }
}
```

#### Refactoring Approach
1. **Identify**: Find components with direct API calls (`invoke()`, `fetch()`)
2. **Abstract**: Create service interface matching the functionality
3. **Implement**: Add both local (Tauri) and remote implementations
4. **Inject**: Use dependency injection to provide appropriate service
5. **Test**: Verify both service paths work correctly

#### Migration Strategy
- **Phase 1**: Extract service interfaces without changing behavior
- **Phase 2**: Implement dependency injection container
- **Phase 3**: Migrate components to use service hooks
- **Phase 4**: Add comprehensive error handling and offline support

#### Key Success Indicators
- [ ] No direct `invoke()` calls in React components
- [ ] All services implement consistent interfaces
- [ ] Clear separation between local and remote service implementations
- [ ] Components are testable with mock services
- [ ] Offline functionality works seamlessly

#### When Adding New Features
1. **Design the service interface first** - What operations does this feature need?
2. **Implement both local and remote versions** - Even if you only need one initially
3. **Create React hooks for UI integration** - Keep components simple
4. **Add proper error handling** - Network failures, permission errors, etc.
5. **Document the service selection rationale** - Why Rust vs Frontend?

This architecture enables the application to be **offline-first**, **testable**, **maintainable**, and ready for **multi-platform deployment** while maintaining clear separation of concerns.
