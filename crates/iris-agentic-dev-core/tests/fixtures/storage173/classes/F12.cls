Class IadSm173.F12 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property A As %String;

Property B As %Integer;

Property Name As %String;

Property City As %String;

Index IDKEY On (A, B) [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Data name="City">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Data name="Name">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Global>^IadSm173F12</Global>
<RowIdSpec name="1">
<Expression>{L1}</Expression>
<Field>A</Field>
</RowIdSpec>
<RowIdSpec name="2">
<Expression>{L2}</Expression>
<Field>B</Field>
</RowIdSpec>
<Subscript name="1">
<Expression>{A}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{B}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<SQLMap name="X">
<Global>^IadSm173F12X</Global>
<Subscript name="1">
<Expression>$$SQLUPPER({Name})</Expression>
</Subscript>
<Subscript name="2">
<Expression>{A}</Expression>
</Subscript>
<Subscript name="3">
<Expression>{B}</Expression>
</Subscript>
<Type>index</Type>
</SQLMap>
<StreamLocation>^IadSm173F12S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
