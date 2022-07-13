exports.handler =  async (event, context) => {
    var current = new Date();
    var time = current.toLocaleDateString() + "-" + current.getHours() + ":" + current.getMinutes() + ":"
        + current.getSeconds() + ":" + current.getMilliseconds();
    time = time.replace(/\//g, '-');
    return ({
        timestamp: time
    });
}
