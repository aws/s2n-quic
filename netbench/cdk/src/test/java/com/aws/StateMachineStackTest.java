package com.aws;

import software.amazon.awscdk.App;
import software.amazon.awscdk.assertions.Template;
import java.io.IOException;
import org.junit.jupiter.api.Test;
import java.util.HashMap;
import software.amazon.awscdk.Environment;
import java.util.List;
import java.util.Map;

/* Unit testing for StateMachineStack, tests for key properties
 * in the cloudformation output of the stack.
 * Some empty maps and lists can be found to match
 * any value for fields in the Cloudformation output with
 * unimportant information.
 * More extensive tests are performed through deployments
 * of various scenarios and verifying outputs.
 */
public class StateMachineStackTest {
    private String cidr;
    private String region;
    private String instanceType;
    private String serverEcrUri;
    private String clientEcrUri;
    private String scenarioFile;
    private Template smTemplate;
    private EcsStack clientEcsStack;
    private EcsStack serverEcsStack;
    private VpcStack vpcStack;
    private StateMachineStack stateMachineStack;
    private String protocol;

    public StateMachineStackTest() {
        App app = new App();

        protocol = "s2n-quic";
        cidr = "11.0.0.0/16";
        region = "us-west-2";
        instanceType = "t4g.xlarge";
        serverEcrUri = "public.ecr.aws/d2r9y8c2/s2n-quic-collector-server-scenario";
        clientEcrUri = "public.ecr.aws/d2r9y8c2/s2n-quic-collector-client-scenario";
        scenarioFile = "/usr/bin/request_response.json";

        vpcStack = new VpcStack(app, "VpcStack", VpcStackProps.builder()
            .env(makeEnv(System.getenv("CDK_DEFAULT_ACCOUNT"), region))
            .cidr(cidr)
            .build());

        serverEcsStack = new EcsStack(app, "ServerEcsStack", EcsStackProps.builder()
            .env(makeEnv(System.getenv("CDK_DEFAULT_ACCOUNT"), region))
            .bucket(vpcStack.getBucket())
            .stackType("server")
            .vpc(vpcStack.getVpc())
            .instanceType(instanceType)
            .ecrUri(serverEcrUri)
            .scenario(scenarioFile)
            .serverRegion(region)
            .build());
        
        clientEcsStack = new EcsStack(app, "ClientEcsStack", EcsStackProps.builder()
            .env(makeEnv(System.getenv("CDK_DEFAULT_ACCOUNT"), region))
            .bucket(vpcStack.getBucket())
            .stackType("client")
            .vpc(vpcStack.getVpc())
            .instanceType(instanceType)
            .serverRegion(region)
            .dnsAddress(serverEcsStack.getDnsAddress())
            .ecrUri(clientEcrUri)
            .scenario(scenarioFile)
            .build());
        
        stateMachineStack = new StateMachineStack(app, "StateMachineStack", StateMachineStackProps.builder()
            .env(makeEnv(System.getenv("CDK_DEFAULT_ACCOUNT"), region))
            .clientTask(clientEcsStack.getEcsTask())
            .bucket(vpcStack.getBucket())
            .logsLambda(serverEcsStack.getLogsLambda())
            .cluster(clientEcsStack.getCluster())
            .protocol(protocol)
            .build());

        smTemplate = Template.fromStack(stateMachineStack);
    }


    @Test
     public void testStack() throws IOException {
        
        //Timestamp function
        smTemplate.hasResourceProperties("AWS::Lambda::Function", new HashMap<String, Object>() {{
            put("Handler", "timestamp.handler");
            put("Runtime", "nodejs14.x");
        }});

        //Log retention for timestamp function
        smTemplate.hasResourceProperties("Custom::LogRetention", new HashMap<String, Object>() {{
            put("RetentionInDays", 1);
        }});

        //Report Generation Task
        smTemplate.hasResourceProperties("AWS::ECS::TaskDefinition", new HashMap<String, Object>() {{
            put("ContainerDefinitions", List.of(Map.of(
                "Environment", List.of(Map.of(), Map.of("Name", "PROTOCOL", "Value", "s2n-quic")),
                "Name", "report-generation"
            )));
        }});

        //Report Generation Task Policy
        smTemplate.hasResourceProperties("AWS::IAM::Policy", new HashMap<String, Object>() {{
            put("PolicyDocument", Map.of("Statement", List.of(
                Map.of("Action", "logs:DescribeExportTasks", "Effect", "Allow", "Resource", "*"), 
                Map.of("Action", List.of("s3:GetObject*",
                "s3:GetBucket*",
                "s3:List*",
                "s3:DeleteObject*",
                "s3:PutObject",
                "s3:PutObjectLegalHold",
                "s3:PutObjectRetention",
                "s3:PutObjectTagging",
                "s3:PutObjectVersionTagging",
                "s3:Abort*"),
                "Effect", "Allow")
            )));
        }});

        //State Machine
        smTemplate.hasResourceProperties("AWS::StepFunctions::StateMachine", new HashMap<String, Object>() {{}});
     }

     static Environment makeEnv(String account, String region) {
      return Environment.builder()
          .account(account)
          .region(region)
          .build();
  }
}
