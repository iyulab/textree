using System.Reflection;
using Textree.Host.Telemetry;
using Xunit;

public class TelemetryEventNameTests
{
    [Fact]
    public void All_contains_every_declared_event_constant()
    {
        var declared = typeof(TelemetryEventName)
            .GetFields(BindingFlags.Public | BindingFlags.Static)
            .Where(f => f.IsLiteral && f.FieldType == typeof(string))
            .Select(f => (string)f.GetValue(null)!)
            .ToList();

        Assert.NotEmpty(declared);
        Assert.All(declared, name => Assert.Contains(name, TelemetryEventName.All));
    }
}
