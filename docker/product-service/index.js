const { ApolloServer } = require('@apollo/server');
const { startStandaloneServer } = require('@apollo/server/standalone');
const { v4: uuidv4 } = require('uuid');
const { readFileSync } = require('fs');
const path = require('path');

// Load schema from file
const typeDefs = readFileSync(path.join(__dirname, 'schema.graphql'), 'utf8');

// In-memory database
let products = [
  { id: '1', name: 'Laptop', price: 999.99 },
  { id: '2', name: 'Smartphone', price: 699.99 },
];

// Resolvers
const resolvers = {
    Query: {
      product: (_, { id }) => products.find(product => product.id === id),
      products: () => products
    },
    Mutation: {
      createProduct: (_, { input }) => {
        const newProduct = {
          id: uuidv4(),
          ...input
        };
        products.push(newProduct);
        return newProduct;
      },
      deleteProduct: (_, { id }) => {
        const productIndex = products.findIndex(product => product.id === id);
        if (productIndex === -1) {
          return {
            success: false,
            message: `Product with ID ${id} not found`
          };
        }

        products.splice(productIndex, 1);
        return {
          success: true,
          message: `Product with ID ${id} successfully deleted`
        };
      }
    }
  };

// Create and start the server
async function startServer() {
  const server = new ApolloServer({
    typeDefs,
    resolvers,
  });

  const { url } = await startStandaloneServer(server, {
    listen: { port: 4001 },
  });

  console.log(`🚀 Product service ready at ${url}`);
}

startServer();
