using System.Text.Json;
using ModelContextProtocol.Protocol;
using ModelContextProtocol.Server;

namespace Textree.Mcp;

/// <summary>
/// Checks the arguments of a tool call against the tool's input schema before the call, so a
/// missing or mistyped argument comes back as a tool error that says which and why
/// (<c>error: missing argument: path</c>) rather than a generic failure.
/// </summary>
public static class Arguments
{
    public static string? Problem(Tool tool, IDictionary<string, JsonElement>? arguments)
    {
        var schema = tool.InputSchema;
        if (schema.ValueKind != JsonValueKind.Object)
        {
            return null;
        }
        var given = arguments ?? new Dictionary<string, JsonElement>();
        if (schema.TryGetProperty("required", out var required) && required.ValueKind == JsonValueKind.Array)
        {
            foreach (var name in required.EnumerateArray().Select(r => r.GetString()).OfType<string>())
            {
                if (!given.TryGetValue(name, out var value) || value.ValueKind == JsonValueKind.Null)
                {
                    return $"error: missing argument: {name}";
                }
            }
        }
        var properties = schema.TryGetProperty("properties", out var p) && p.ValueKind == JsonValueKind.Object ? p : default;
        foreach (var (name, value) in given)
        {
            if (properties.ValueKind != JsonValueKind.Object || !properties.TryGetProperty(name, out var property))
            {
                return $"error: unknown argument: {name}";
            }
            if (value.ValueKind == JsonValueKind.Null)
            {
                continue;
            }
            var types = TypesOf(property);
            if (types.Count > 0 && !types.Any(t => Fits(t, value)))
            {
                return $"error: {name}: expected {string.Join(" or ", types.Where(t => t != "null"))}";
            }
        }
        return null;
    }

    private static List<string> TypesOf(JsonElement property)
    {
        if (!property.TryGetProperty("type", out var type))
        {
            return [];
        }
        return type.ValueKind switch
        {
            JsonValueKind.String => [type.GetString()!],
            JsonValueKind.Array => type.EnumerateArray().Select(t => t.GetString()).OfType<string>().ToList(),
            _ => [],
        };
    }

    private static bool Fits(string type, JsonElement value) => type switch
    {
        "string" => value.ValueKind == JsonValueKind.String,
        "integer" => value.ValueKind == JsonValueKind.Number && value.TryGetInt64(out _),
        "number" => value.ValueKind == JsonValueKind.Number,
        "boolean" => value.ValueKind is JsonValueKind.True or JsonValueKind.False,
        "array" => value.ValueKind == JsonValueKind.Array,
        "object" => value.ValueKind == JsonValueKind.Object,
        "null" => value.ValueKind == JsonValueKind.Null,
        _ => true,
    };
}
