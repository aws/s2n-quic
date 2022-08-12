package com.aws;

import software.amazon.awscdk.App;
import software.amazon.awscdk.assertions.Template;
import java.io.IOException;
import org.junit.jupiter.api.Test;
import java.util.HashMap;
import software.amazon.awscdk.Environment;
import java.util.List;
import java.util.Map;


/* Unit testing for ClientEcsStack, tests for key properties
 * in the cloudformation output of the stack.
 * Some empty maps and lists can be found to match
 * any value for fields in the Cloudformation output with
 * unimportant information.
 * More extensive tests are performed through deployments
 * of various scenarios and verifying outputs.
 */
public class ClientEcsStackTest {
    private String cidr;
    private String region;
    private String instanceType;
    private String serverEcrUri;
    private String clientEcrUri;
    private String scenarioFile;
    private Template clientTemplate;
    private EcsStack clientEcsStack;
    private EcsStack serverEcsStack;
    private VpcStack vpcStack;
    private String arm;

    public ClientEcsStackTest() {
        App app = new App();

        cidr = "11.0.0.0/16";
        region = "us-west-2";
        instanceType = "t4g.xlarge";
        serverEcrUri = "public.ecr.aws/d2r9y8c2/s2n-quic-collector-server-scenario";
        clientEcrUri = "public.ecr.aws/d2r9y8c2/s2n-quic-collector-client-scenario";
        scenarioFile = "/usr/bin/request_response.json";
        arm = "true";

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
            .arm(arm)
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
            .arm(arm)
            .build());
        
        clientTemplate = Template.fromStack(clientEcsStack);
    }

    @Test
     public void testStack() throws IOException {

        //Security Group
        clientTemplate.hasResourceProperties("AWS::EC2::SecurityGroup", new HashMap<String, Object>() {{
            put("SecurityGroupEgress", List.of(Map.of("CidrIp", "0.0.0.0/0", "Description", "Allow all outbound traffic by default","IpProtocol", "-1")));
            put("SecurityGroupIngress", List.of(Map.of("CidrIp", "0.0.0.0/0", "Description", "from 0.0.0.0/0:ALL TRAFFIC","IpProtocol", "-1")));
        }});

        //Cluster
        clientTemplate.hasResource("AWS::ECS::Cluster", new HashMap<String, Object>() {{
            put("DeletionPolicy", "Delete");
        }});
        
        //Autoscaling group
        clientTemplate.hasResourceProperties("AWS::AutoScaling::AutoScalingGroup", new HashMap<String, Object>() {{
            put("DesiredCapacity", "1");
            put("MinSize", "0");
        }});

        //Asg Provider
        clientTemplate.hasResourceProperties("AWS::ECS::CapacityProvider", new HashMap<String, Object>() {{
            put("AutoScalingGroupProvider", Map.of("ManagedTerminationProtection","DISABLED"));
        }});

        //Launch Configuration
        clientTemplate.hasResourceProperties("AWS::AutoScaling::LaunchConfiguration", new HashMap<String, Object>() {{
            put("InstanceType", instanceType);
        }});

        //Task Definition
        clientTemplate.hasResourceProperties("AWS::ECS::TaskDefinition", new HashMap<String, Object>() {{
            put("ContainerDefinitions", List.of(Map.of(
                "Environment", List.of(Map.of("Name", "SERVER_PORT", "Value", "3000"),
                    Map.of("Name", "S3_BUCKET", "Value", Map.of()), 
                    Map.of("Name", "PORT", "Value", "3000"),
                    Map.of("Name", "LOCAL_IP", "Value", "0.0.0.0"),
                    Map.of("Name", "SCENARIO", "Value", "/usr/bin/request_response.json"),
                    Map.of("Name", "DNS_ADDRESS", "Value", "ec2serviceserverCloudmapSrv-UEyneXTpp1nx.serverecs.com")),
                "PortMappings", List.of(Map.of("ContainerPort", 3000, "HostPort", 3000, "Protocol", "udp")),
                "LogConfiguration", Map.of("LogDriver", "awslogs"))
            ));
            put("NetworkMode", "awsvpc");
        }});

        //Client task policy to access metrics s3 bucket
        clientTemplate.hasResourceProperties("AWS::IAM::Policy", new HashMap<String, Object>() {{
            put("PolicyDocument", Map.of("Statement", 
            List.of(Map.of("Action", 
                List.of(
                "s3:DeleteObject*",
                "s3:PutObject",
                "s3:PutObjectLegalHold",
                "s3:PutObjectRetention",
                "s3:PutObjectTagging",
                "s3:PutObjectVersionTagging",
                "s3:Abort*"),
                "Effect", "Allow",
                "Resource", List.of(Map.of(), Map.of()))),
                "Version", "2012-10-17"));
        }});

        //Client task policy can send logs to Cloudwatch
        clientTemplate.hasResourceProperties("AWS::IAM::Policy", new HashMap<String, Object>() {{
            put("PolicyDocument", Map.of("Statement", 
            List.of(Map.of("Action", 
                List.of(
                "logs:CreateLogStream",
                "logs:PutLogEvents"),
                "Effect", "Allow",
                "Resource", Map.of()))));
        }});

        //LogGroup
        clientTemplate.hasResourceProperties("AWS::Logs::LogGroup", new HashMap<String, Object>() {{
            put("RetentionInDays", 1);
        }});

     }

     static Environment makeEnv(String account, String region) {
      return Environment.builder()
          .account(account)
          .region(region)
          .build();
  }
}
