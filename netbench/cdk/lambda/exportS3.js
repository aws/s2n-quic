const AWS = require('aws-sdk')
const cloudconfig = {
  apiVersion: '2014-03-28',
  region: process.env.AWS_REGION, 
}
const cloudwatchlogs = new AWS.CloudWatchLogs(cloudconfig)
exports.handler =  async (event, context) => {
   const params = {
    destination: process.env.BUCKET_NAME, 
    from: new Date().getTime() - 6000000, //TODO: will be replaced with the timestamp of the start of statemachine execution
    logGroupName: process.env.LOG_GROUP_NAME,
    to: new Date().getTime(),
    destinationPrefix: event.Payload.timestamp
  };
  console.log(params);

  var data = await cloudwatchlogs.createExportTask(params).promise();
  console.log(data);
  return data;
}

