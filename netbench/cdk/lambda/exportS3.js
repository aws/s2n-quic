const AWS = require('aws-sdk')
const cloudconfig = {
  apiVersion: '2014-03-28',
  region: process.env.REGION, 
}
const cloudwatchlogs = new AWS.CloudWatchLogs(cloudconfig)
exports.handler =  async (event, context) => {
   const params = {
    destination: process.env.BUCKET_NAME, 
    from: new Date().getTime() - 6000000, //ten minutes, will add exact time stamp in future iteration
    logGroupName: process.env.LOG_GROUP_NAME,
    to: new Date().getTime()
  };

var data = await cloudwatchlogs.createExportTask(params).promise();

return data;
}

