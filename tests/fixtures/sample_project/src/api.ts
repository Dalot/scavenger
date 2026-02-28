interface User {
  id: number;
  name: string;
  email: string;
}

interface ApiResponse<T> {
  data: T;
  status: number;
  message: string;
}

type UserId = number;

enum Role {
  Admin,
  User,
  Guest,
}

function fetchUser(id: UserId): Promise<ApiResponse<User>> {
  return fetch(`/api/users/${id}`).then((res) => res.json());
}

function createUser(name: string, email: string): Promise<ApiResponse<User>> {
  return fetch("/api/users", {
    method: "POST",
    body: JSON.stringify({ name, email }),
  }).then((res) => res.json());
}

class UserService {
  private baseUrl: string;

  constructor(baseUrl: string) {
    this.baseUrl = baseUrl;
  }

  async getUser(id: number): Promise<User> {
    const response = await fetchUser(id);
    return response.data;
  }
}
