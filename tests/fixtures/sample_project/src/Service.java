package com.example;

/**
 * Base service interface for all application services.
 */
interface ServiceBase {
    String getName();
    void initialize();
}

/**
 * User management service.
 */
class UserService implements ServiceBase {
    private String name;

    public UserService(String name) {
        this.name = name;
    }

    public String getName() {
        return this.name;
    }

    public void initialize() {
        System.out.println("Initializing " + name);
    }

    /**
     * Find a user by their unique identifier.
     */
    public Object findUser(int id) {
        return null;
    }

    public void deleteUser(int id) {
        System.out.println("Deleting user " + id);
    }
}

enum Permission {
    READ,
    WRITE,
    ADMIN
}
