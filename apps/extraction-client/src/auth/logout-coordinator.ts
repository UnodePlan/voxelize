import type { AuthApi } from "../api/auth";

export class LogoutCoordinator {
  private request: Promise<boolean> | null = null;

  constructor(private readonly auth: AuthApi) {}

  activate(): void {
    this.request = null;
  }

  logout(): Promise<boolean> {
    this.request ??= this.auth.logout().then(
      () => {
        return true;
      },
      () => false,
    );
    return this.request;
  }
}
