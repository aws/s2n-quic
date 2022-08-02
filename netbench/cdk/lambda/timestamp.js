exports.handler =  async (event, context) => {
    return ({
        timestamp: (new Date).toISOString()
    });
}
