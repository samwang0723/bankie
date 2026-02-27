import {
  createContext,
  useContext,
  useState,
  useCallback,
  useEffect,
  type ReactNode,
} from 'react';
import { createElement } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, ApiClientError } from '../api/client.ts';
import type {
  AuthResponse,
  LoginRequest,
  SignupRequest,
  PortalUser,
  Organization,
} from '../types/index.ts';

interface AuthState {
  user: PortalUser | null;
  organization: Organization | null;
  token: string | null;
  isAuthenticated: boolean;
}

interface AuthContextValue extends AuthState {
  login: (credentials: LoginRequest) => Promise<void>;
  signup: (data: SignupRequest) => Promise<void>;
  logout: () => void;
}

const AuthContext = createContext<AuthContextValue | null>(null);

function loadPersistedAuth(): AuthState {
  const token = localStorage.getItem('auth_token');
  const userJson = localStorage.getItem('auth_user');
  const orgJson = localStorage.getItem('auth_org');

  if (token && userJson && orgJson) {
    try {
      return {
        token,
        user: JSON.parse(userJson),
        organization: JSON.parse(orgJson),
        isAuthenticated: true,
      };
    } catch {
      // Corrupt data, clear it
      localStorage.removeItem('auth_token');
      localStorage.removeItem('auth_user');
      localStorage.removeItem('auth_org');
    }
  }

  return { user: null, organization: null, token: null, isAuthenticated: false };
}

function persistAuth(response: AuthResponse): void {
  localStorage.setItem('auth_token', response.token);
  localStorage.setItem('auth_user', JSON.stringify(response.user));
  localStorage.setItem('auth_org', JSON.stringify(response.organization));
}

function clearPersistedAuth(): void {
  localStorage.removeItem('auth_token');
  localStorage.removeItem('auth_user');
  localStorage.removeItem('auth_org');
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AuthState>(loadPersistedAuth);
  const navigate = useNavigate();

  const login = useCallback(async (credentials: LoginRequest) => {
    const response = await api.post<AuthResponse>('/auth/login', credentials);
    persistAuth(response);
    setState({
      user: response.user,
      organization: response.organization,
      token: response.token,
      isAuthenticated: true,
    });
  }, []);

  const signup = useCallback(async (data: SignupRequest) => {
    const response = await api.post<AuthResponse>('/auth/signup', data);
    persistAuth(response);
    setState({
      user: response.user,
      organization: response.organization,
      token: response.token,
      isAuthenticated: true,
    });
  }, []);

  const logout = useCallback(() => {
    clearPersistedAuth();
    setState({ user: null, organization: null, token: null, isAuthenticated: false });
    navigate('/login');
  }, [navigate]);

  // Global 401 handler — listen for unauthorized errors
  useEffect(() => {
    const handleUnauthorized = (event: Event) => {
      if (event instanceof CustomEvent && event.detail?.status === 401) {
        logout();
      }
    };
    window.addEventListener('auth:unauthorized', handleUnauthorized);
    return () => window.removeEventListener('auth:unauthorized', handleUnauthorized);
  }, [logout]);

  const value: AuthContextValue = {
    ...state,
    login,
    signup,
    logout,
  };

  return createElement(AuthContext.Provider, { value }, children);
}

export function useAuth(): AuthContextValue {
  const context = useContext(AuthContext);
  if (!context) {
    throw new Error('useAuth must be used within an AuthProvider');
  }
  return context;
}

// Utility: check if an error is a 401 and dispatch event
export function handleApiError(error: unknown): never {
  if (error instanceof ApiClientError && error.status === 401) {
    window.dispatchEvent(
      new CustomEvent('auth:unauthorized', { detail: { status: 401 } }),
    );
  }
  throw error;
}
