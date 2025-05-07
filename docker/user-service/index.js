const { ApolloServer } = require("@apollo/server");
const { startStandaloneServer } = require("@apollo/server/standalone");
const { v4: uuidv4 } = require('uuid');
const { readFileSync } = require("fs");
const path = require("path");

// Load schema from file
const typeDefs = readFileSync(path.join(__dirname, "schema.graphql"), "utf8");

// In-memory database
let users = [
    { id: "1", name: "John Doe", email: "john@example.com" },
    { id: "2", name: "Jane Smith", email: "jane@example.com" },
];

// Resolvers
const resolvers = {
    Query: {
        user: (_, { id }) => users.find((user) => user.id === id),
        users: () => users,
    },
    Mutation: {
        createUser: (_, { input }) => {
            const newUser = {
                id: uuidv4(),
                ...input,
            };
            users.push(newUser);
            return newUser;
        },
        deleteUser: (_, { id }) => {
            const userIndex = users.findIndex((user) => user.id === id);
            if (userIndex === -1) {
                return {
                    success: false,
                    message: `User with ID ${id} not found`,
                };
            }

            users.splice(userIndex, 1);
            return {
                success: true,
                message: `User with ID ${id} successfully deleted`,
            };
        },
    },
};

// Create and start the server
async function startServer() {
    const server = new ApolloServer({
        typeDefs,
        resolvers,
    });

    const { url } = await startStandaloneServer(server, {
        listen: { port: 4000 },
    });

    console.log(`🚀 User service ready at ${url}`);
}

startServer();
